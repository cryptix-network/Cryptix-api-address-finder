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


---- 

It works very simply: you enter a wallet address or part of an address, for example “vpnz” as the ending of a censored address. Then you specify the hot wallet of a pool or an exchange. The script then scans all transactions and returns the full address or the related transactions.

This allows you to automatically find addresses, verify transactions, and maintain full transparency—even for anonymized addresses.



Example: Finding an anonymized address

There is a precompiled Windows .exe, so you only need to open the start .bat file. After starting it, enter the hot wallet / pool wallet. Then press 1.

Next, enter the letters of the wallet that you already know, for example “vpnz”. Then wait a moment and the script will return the full address.

Source code for Linux, or you can compile it yourself, is also available. It's a simple little Rust script.

https://github.com/cryptix-network/Cryptix-api-address-finder
