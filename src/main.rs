use anyhow::{anyhow, Context, Result};
use reqwest::{redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

const CONFIG_PATH: &str = "config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    base_url: String,
    limit: u32,
    page_delay_seconds: u64,
    request_timeout_seconds: u64,
    #[serde(default)]
    allow_invalid_certs: bool,
    retry: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetryConfig {
    enabled: bool,
    retry_delay_seconds: u64,
    max_attempts: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: "https://rest.seed1.cryptix-network.org".to_string(),
            limit: 400,
            page_delay_seconds: 10,
            request_timeout_seconds: 30,
            allow_invalid_certs: false,
            retry: RetryConfig {
                enabled: true,
                retry_delay_seconds: 5,
                max_attempts: 0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    FullAddressUntilFirstHit,
    FullAddressDepth(u64),
    TransactionsUntilFirstHit,
    TransactionsDepth(u64),
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_or_create_config(CONFIG_PATH)?;

    println!("Cryptix API Address Finder");
    println!();

    let source_wallet = prompt("1) Source wallet address to scan (full): ")?;
    if source_wallet.trim().is_empty() {
        return Err(anyhow!("Source wallet address must not be empty."));
    }

    let needle = prompt("2) Search term (wallet address or partial text, e.g. z7lo9): ")?;
    if needle.trim().is_empty() {
        return Err(anyhow!("Search term must not be empty."));
    }

    println!();
    println!("3) Select mode:");
    println!("1. Scan for full address (until first hit)");
    println!("2. Scan for full address (fixed depth)");
    println!("3. Scan for transactions (until first hit)");
    println!("4. Scan for transactions (fixed depth)");

    let mode = loop {
        let choice = prompt("Choice (1-4): ")?;
        match choice.trim() {
            "1" => break Mode::FullAddressUntilFirstHit,
            "2" => {
                let depth = prompt_u64("Depth in pages (offset starts at 0): ")?;
                if depth == 0 {
                    println!("Depth must be > 0.");
                    continue;
                }
                break Mode::FullAddressDepth(depth);
            }
            "3" => break Mode::TransactionsUntilFirstHit,
            "4" => {
                let depth = prompt_u64("Depth in pages (offset starts at 0): ")?;
                if depth == 0 {
                    println!("Depth must be > 0.");
                    continue;
                }
                break Mode::TransactionsDepth(depth);
            }
            _ => println!("Invalid choice. Please enter 1, 2, 3, or 4."),
        }
    };

    println!();
    println!(
        "Start: source={}, needle={}, base_url={}, limit={}, delay={}s",
        source_wallet, needle, config.base_url, config.limit, config.page_delay_seconds
    );
    println!();

    run_scan(&config, &source_wallet, &needle, mode).await
}

fn load_or_create_config(path: &str) -> Result<Config> {
    let config_path = Path::new(path);
    if !config_path.exists() {
        let default_cfg = Config::default();
        let data =
            toml::to_string_pretty(&default_cfg).context("Failed to serialize default config.")?;
        fs::write(config_path, data).with_context(|| format!("Failed to write {}.", path))?;
        println!("Config {} was created. Using default values.", path);
    }

    let raw = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}.", path))?;
    let config: Config =
        toml::from_str(&raw).with_context(|| format!("Invalid TOML in {}.", path))?;
    Ok(config)
}

async fn run_scan(config: &Config, source_wallet: &str, needle: &str, mode: Mode) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(config.request_timeout_seconds))
        .connect_timeout(Duration::from_secs(config.request_timeout_seconds))
        .user_agent("cryptix-address-finder/1.0")
        .redirect(Policy::limited(10))
        .danger_accept_invalid_certs(config.allow_invalid_certs)
        .build()
        .context("Failed to create HTTP client.")?;

    if config.allow_invalid_certs {
        println!("WARNING: TLS certificate verification is disabled (allow_invalid_certs=true).");
    }

    let needle_lc = needle.to_lowercase();
    let mut offset: u64 = 0;
    let max_pages = match mode {
        Mode::FullAddressDepth(depth) | Mode::TransactionsDepth(depth) => Some(depth),
        Mode::FullAddressUntilFirstHit | Mode::TransactionsUntilFirstHit => None,
    };

    let mut collected_addresses: HashSet<String> = HashSet::new();
    let mut collected_transactions: Vec<Value> = Vec::new();

    loop {
        if let Some(max) = max_pages {
            if offset >= max {
                break;
            }
        }

        println!("Scanning offset={} ...", offset);
        let page = fetch_page(config, &client, source_wallet, offset).await?;

        if page.is_empty() {
            println!("No more transactions on this page. Scan finished.");
            break;
        }

        let mut page_hits = 0usize;
        for tx in page {
            let matches = matching_addresses(&tx, &needle_lc);
            if matches.is_empty() {
                continue;
            }

            page_hits += 1;
            match mode {
                Mode::FullAddressUntilFirstHit | Mode::FullAddressDepth(_) => {
                    for addr in matches {
                        collected_addresses.insert(addr);
                    }
                }
                Mode::TransactionsUntilFirstHit | Mode::TransactionsDepth(_) => {
                    collected_transactions.push(tx);
                }
            }
        }

        match mode {
            Mode::FullAddressUntilFirstHit | Mode::FullAddressDepth(_) => {
                if page_hits > 0 {
                    println!(
                        "Hit on offset={}: {} transaction(s), {} unique address(es) collected.",
                        offset,
                        page_hits,
                        collected_addresses.len()
                    );
                }
            }
            Mode::TransactionsUntilFirstHit | Mode::TransactionsDepth(_) => {
                if page_hits > 0 {
                    println!(
                        "Hit on offset={}: {} transaction(s), {} collected.",
                        offset,
                        page_hits,
                        collected_transactions.len()
                    );
                }
            }
        }

        let stop_on_first_hit =
            matches!(mode, Mode::FullAddressUntilFirstHit | Mode::TransactionsUntilFirstHit);

        if stop_on_first_hit {
            let hit_found = match mode {
                Mode::FullAddressUntilFirstHit => !collected_addresses.is_empty(),
                Mode::TransactionsUntilFirstHit => !collected_transactions.is_empty(),
                _ => false,
            };

            if hit_found {
                println!("First hit found. Scan stopped.");
                break;
            }
        }

        offset += 1;
        if let Some(max) = max_pages {
            if offset >= max {
                break;
            }
        }

        println!("Waiting {}s before next page ...", config.page_delay_seconds);
        sleep(Duration::from_secs(config.page_delay_seconds)).await;
    }

    println!();
    match mode {
        Mode::FullAddressUntilFirstHit | Mode::FullAddressDepth(_) => {
            if collected_addresses.is_empty() {
                println!("No matching wallet addresses found.");
            } else {
                let mut list: Vec<String> = collected_addresses.into_iter().collect();
                list.sort_unstable();
                println!("Matching wallet addresses ({}):", list.len());
                for addr in list {
                    println!("- {}", addr);
                }
            }
        }
        Mode::TransactionsUntilFirstHit | Mode::TransactionsDepth(_) => {
            if collected_transactions.is_empty() {
                println!("No matching transactions found.");
            } else {
                println!("Matching transactions ({}):", collected_transactions.len());
                for tx in collected_transactions {
                    let tx_id = tx_identifier(&tx);
                    println!("--------------------------------------------------");
                    println!("transaction_id/hash: {}", tx_id);
                    println!("{}", serde_json::to_string_pretty(&tx)?);
                }
            }
        }
    }

    Ok(())
}

async fn fetch_page(
    config: &Config,
    client: &Client,
    source_wallet: &str,
    offset: u64,
) -> Result<Vec<Value>> {
    let encoded_wallet = urlencoding::encode(source_wallet);
    let urls = request_urls(config, &encoded_wallet, offset);
    if urls.is_empty() {
        return Err(anyhow!("Invalid base_url in config.toml: '{}'", config.base_url));
    }

    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let mut network_errors: Vec<String> = Vec::new();

        for url in &urls {
            println!("Request URL: {}", url);

            let response = client.get(url).send().await;
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let data = resp
                        .json::<Vec<Value>>()
                        .await
                        .with_context(|| format!("Invalid JSON response at offset={}", offset))?;
                    return Ok(data);
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| "<no response details>".to_string());

                    let err = anyhow!(
                        "HTTP error at offset={}: status={}, body={}",
                        offset,
                        status,
                        body
                    );

                    if should_retry(config, attempts) {
                        println!(
                            "Error: {:#}. Retrying in {}s (attempt {}).",
                            err, config.retry.retry_delay_seconds, attempts
                        );
                        sleep(Duration::from_secs(config.retry.retry_delay_seconds)).await;
                        break;
                    }

                    return Err(err);
                }
                Err(err) => {
                    network_errors.push(format!(
                        "{} -> {}",
                        url,
                        format_reqwest_error(&err)
                    ));
                }
            }
        }

        if !network_errors.is_empty() {
            if should_retry(config, attempts) {
                println!(
                    "Network error at offset={}. Retrying in {}s (attempt {}).",
                    offset, config.retry.retry_delay_seconds, attempts
                );
                for detail in &network_errors {
                    println!("  {}", detail);
                }
                sleep(Duration::from_secs(config.retry.retry_delay_seconds)).await;
                continue;
            }

            return Err(anyhow!(
                "Network error at offset={}. Tried:\n{}",
                offset,
                network_errors.join("\n")
            ));
        }
    }
}

fn request_urls(config: &Config, encoded_wallet: &str, offset: u64) -> Vec<String> {
    let base = config.base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Vec::new();
    }

    let suffix = format!(
        "/addresses/{}/full-transactions?limit={}&offset={}",
        encoded_wallet, config.limit, offset
    );

    if let Some(rest) = base.strip_prefix("https://") {
        let https = format!("https://{}{}", rest, suffix);
        let http = format!("http://{}{}", rest, suffix);
        vec![https, http]
    } else if let Some(rest) = base.strip_prefix("http://") {
        let https = format!("https://{}{}", rest, suffix);
        let http = format!("http://{}{}", rest, suffix);
        vec![https, http]
    } else {
        let https = format!("https://{}{}", base, suffix);
        let http = format!("http://{}{}", base, suffix);
        vec![https, http]
    }
}

fn format_reqwest_error(err: &reqwest::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(next) = source {
        parts.push(next.to_string());
        source = next.source();
    }
    parts.join(" | caused by: ")
}

fn should_retry(config: &Config, attempts: u32) -> bool {
    if !config.retry.enabled {
        return false;
    }

    if config.retry.max_attempts == 0 {
        return true;
    }

    attempts < config.retry.max_attempts
}

fn matching_addresses(tx: &Value, needle_lc: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    collect_addresses(tx, &mut addresses);

    let mut seen = HashSet::new();
    addresses
        .into_iter()
        .filter(|addr| addr.to_lowercase().contains(needle_lc))
        .filter(|addr| seen.insert(addr.clone()))
        .collect()
}

fn collect_addresses(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let is_address_field = k == "script_public_key_address" || k.ends_with("_address");
                if is_address_field {
                    if let Some(s) = v.as_str() {
                        out.push(s.to_string());
                    }
                }
                collect_addresses(v, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_addresses(item, out);
            }
        }
        _ => {}
    }
}

fn tx_identifier(tx: &Value) -> String {
    tx.get("transaction_id")
        .and_then(Value::as_str)
        .or_else(|| tx.get("hash").and_then(Value::as_str))
        .unwrap_or("<unknown>")
        .to_string()
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().context("stdout flush failed")?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).context("stdin read failed")?;
    Ok(buf.trim().to_string())
}

fn prompt_u64(label: &str) -> Result<u64> {
    loop {
        let value = prompt(label)?;
        match value.parse::<u64>() {
            Ok(v) => return Ok(v),
            Err(_) => println!("Please enter a valid number."),
        }
    }
}
