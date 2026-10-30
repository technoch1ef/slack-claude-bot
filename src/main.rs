use axum::{body::Bytes, extract::State, http::HeaderMap, http::StatusCode, routing::post, Router};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

struct Config {
    slack_token: String,
    signing_secret: String,
    anthropic_key: String,
    model: String,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() {
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("missing env var {k}"));
    let cfg = Arc::new(Config {
        slack_token: env("SLACK_BOT_TOKEN"),
        signing_secret: env("SLACK_SIGNING_SECRET"),
        anthropic_key: env("ANTHROPIC_API_KEY"),
        model: std::env::var("CLAUDE_MODEL").unwrap_or_else(|_| "claude-sonnet-5-5".into()),
        http: reqwest::Client::new(),
    });
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let app = Router::new().route("/slack/events", post(events)).with_state(cfg);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn events(State(cfg): State<Arc<Config>>, headers: HeaderMap, body: Bytes) -> (StatusCode, String) {
    if !verify(&cfg.signing_secret, &headers, &body) {
        return (StatusCode::UNAUTHORIZED, String::new());
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, String::new()),
    };
    if payload["type"] == "url_verification" {
        return (StatusCode::OK, payload["challenge"].as_str().unwrap_or("").to_string());
    }
    // Slack redelivers if we take >3s to ack; the first delivery is already being handled.
    if headers.contains_key("x-slack-retry-num") {
        return (StatusCode::OK, String::new());
    }
    if payload["event"]["type"] == "app_mention" {
        tokio::spawn(answer(cfg, payload["event"].clone()));
    }
    (StatusCode::OK, String::new())
}

fn verify(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let header = |k| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
    let ts = header("x-slack-request-timestamp");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    if (now - ts.parse::<i64>().unwrap_or(0)).abs() > 300 {
        return false;
    }
    let Some(sig) = header("x-slack-signature").strip_prefix("v0=").and_then(|s| hex::decode(s).ok()) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v0:{ts}:").as_bytes());
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

async fn answer(cfg: Arc<Config>, event: Value) {
    let channel = event["channel"].as_str().unwrap_or_default();
    let thread_ts = event["thread_ts"].as_str().or(event["ts"].as_str()).unwrap_or_default();
    // shortcut: only the mention text is sent, no thread history; add conversations.replies if follow-ups need context
    let text = event["text"].as_str().unwrap_or_default();
    let reply = match ask_claude(&cfg, text).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("claude error: {e}");
            "Sorry, I couldn't get an answer right now.".into()
        }
    };
    let res = cfg
        .http
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(&cfg.slack_token)
        .json(&json!({ "channel": channel, "thread_ts": thread_ts, "text": reply }))
        .send()
        .await;
    if let Err(e) = res {
        eprintln!("slack error: {e}");
    }
}

async fn ask_claude(cfg: &Config, text: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let res: Value = cfg
        .http
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &cfg.anthropic_key)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": cfg.model,
            "max_tokens": 1024,
            "system": "You are a helpful assistant in a Slack workspace. Answer concisely. Use Slack mrkdwn (*bold*, _italic_, `code`), never Markdown headers.",
            "messages": [{ "role": "user", "content": text }]
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res["content"][0]["text"].as_str().ok_or("no text in response")?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed(secret: &str, body: &[u8], ts: i64) -> HeaderMap {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("v0:{ts}:").as_bytes());
        mac.update(body);
        let mut h = HeaderMap::new();
        h.insert("x-slack-request-timestamp", ts.to_string().parse().unwrap());
        h.insert("x-slack-signature", format!("v0={}", hex::encode(mac.finalize().into_bytes())).parse().unwrap());
        h
    }

    #[test]
    fn verify_accepts_valid_and_rejects_tampered_or_stale() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let h = signed("s3cret", b"{}", now);
        assert!(verify("s3cret", &h, b"{}"));
        assert!(!verify("s3cret", &h, b"{ }"));
        assert!(!verify("other", &h, b"{}"));
        assert!(!verify("s3cret", &signed("s3cret", b"{}", now - 600), b"{}"));
        assert!(!verify("s3cret", &HeaderMap::new(), b"{}"));
    }
}
