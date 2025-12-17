# mira-fetchd

A tiny localhost sidecar that fetches pages using a **real browser** (Playwright).

This is the "proper fix" for sites that return **HTTP 403** or require **JavaScript/cookies**.

## Install

```bash
cd /home/peter/Mira/mira-fetchd
npm install
# first-time only (installs browser binaries)
npx playwright install chromium
```

## Run

```bash
npm start
# listens on http://127.0.0.1:7337 by default
```

Environment:
- `MIRA_FETCHD_PORT` (default: `7337`)
- `MIRA_FETCHD_HOST` (default: `127.0.0.1`)
- `MIRA_FETCHD_PROFILE_DIR` (default: `~/.cache/mira-fetchd-profile`)

## API

`POST /fetch`

```json
{
  "url": "https://example.com",
  "max_chars": 20000,
  "selector": "main",
  "wait_until": "domcontentloaded",
  "timeout_ms": 45000,
  "cache_ttl_ms": 300000
}
```

## Hooked into mira-chat

`mira-chat` will:
- use normal HTTP fetch by default
- **auto-fallback to this service on HTTP 403**

Set (optional):
- `MIRA_FETCHD_URL=http://127.0.0.1:7337`

Or call the tool explicitly: `web_fetch_browser`.
