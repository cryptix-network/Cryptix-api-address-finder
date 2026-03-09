# Cryptix API Address Finder

A small Rust CLI I built to scan a Cryptix wallet history and find matching addresses or transactions.

It pulls paginated `full-transactions` data from the API, walks through the JSON, and checks for your search term.

## Quick start

```bash
cargo run --release
```

The app asks for:
1. Source wallet address
2. Search term (full address or partial text)
3. Scan mode

## Config

Settings are in `config.toml`:
- `base_url`
- `limit`
- `page_delay_seconds`
- `request_timeout_seconds`
- `[retry]` options (`max_attempts = 0` means unlimited retries)
