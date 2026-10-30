# slack-claude-bot

Tiny Rust bot: `@mention` it in Slack, it answers in the thread via the Anthropic API.

## Slack app

1. Create an app at <https://api.slack.com/apps> (from scratch).
2. **OAuth & Permissions** → bot scopes: `app_mentions:read`, `chat:write`. Install to workspace, copy the *Bot User OAuth Token*.
3. **Basic Information** → copy the *Signing Secret*.
4. **Event Subscriptions** → enable, request URL `https://bakesy-slack-claude-bot-889638b7594d.herokuapp.com/slack/events`, subscribe to bot event `app_mention`. (Deploy first so the URL check passes.)
5. In Slack: `/invite @YourBot` into a channel, then `@YourBot what is ...`.

## Env vars

| Var | Value |
| --- | --- |
| `SLACK_BOT_TOKEN` | `xoxb-...` |
| `SLACK_SIGNING_SECRET` | from Basic Information |
| `ANTHROPIC_API_KEY` | from <https://console.anthropic.com> |
| `CLAUDE_MODEL` | optional, default `claude-sonnet-5-5` |

Never commit these. Set them with `heroku config:set` or export them locally.

## Heroku

```sh
heroku create <name> --buildpack https://github.com/emk/heroku-buildpack-rust
heroku config:set SLACK_BOT_TOKEN=... SLACK_SIGNING_SECRET=... ANTHROPIC_API_KEY=...
git push heroku main
```

Use a Basic dyno or better. Eco dynos sleep, and Slack gives up after 3s while the dyno wakes.

## Local

```sh
export SLACK_BOT_TOKEN=... SLACK_SIGNING_SECRET=... ANTHROPIC_API_KEY=...
cargo run
```
