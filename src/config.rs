use std::{collections::HashSet, env, net::SocketAddr, path::PathBuf, time::Duration};

use url::Url;

use crate::error::{ExplorerError, Result};

/// Canonical parent evidence is refreshed at least this often for the Wcash tip.
pub const PARENT_EVIDENCE_REFRESH_INTERVAL_SECONDS: i64 = 30;

/// Strict readiness tolerates one missed tip refresh before becoming unavailable.
pub const PARENT_EVIDENCE_MAX_AGE_SECONDS: i64 = PARENT_EVIDENCE_REFRESH_INTERVAL_SECONDS * 2;

/// Runtime configuration sourced from environment variables.
#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub wcash_rpc: RpcConfig,
    pub zcash_rpcs: Vec<RpcConfig>,
    pub network: NetworkConfig,
    pub poll_interval: Duration,
    pub rpc_timeout: Duration,
    pub max_reorg_depth: u64,
    pub evidence_refresh_depth: u64,
    pub migrate: bool,
    pub run_indexer: bool,
    pub parent_chain: String,
    pub parent_genesis_hash: String,
    pub require_parent_quorum: bool,
}

/// One authenticated JSON-RPC endpoint.
#[derive(Clone, Debug)]
pub struct RpcConfig {
    pub label: String,
    pub url: Url,
    pub auth: RpcAuth,
}

/// Supported node authentication methods.
#[derive(Clone)]
pub enum RpcAuth {
    None,
    Basic { username: String, password: String },
    Cookie(PathBuf),
}

/// Network identity, consensus settings, and presentation metadata.
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    pub id: String,
    pub display_name: String,
    /// Mutable presentation ticker; this does not identify the underlying chain.
    pub symbol: String,
    pub decimals: u8,
    pub coinbase_maturity: u32,
    pub target_spacing_seconds: u32,
    pub max_supply_zat: i64,
    pub initial_subsidy_zat: i64,
    pub halving_interval: u64,
    pub first_halving_height: u64,
    pub genesis_hash: String,
    pub parent_explorer_block_url: String,
    pub parent_explorer_tx_url: String,
}

impl Config {
    /// Loads and validates process configuration.
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();
        let bind = env_or("WCASH_EXPLORER_BIND", "127.0.0.1:8080")
            .parse()
            .map_err(|error| ExplorerError::Config(format!("invalid bind address: {error}")))?;
        let database_url = required("DATABASE_URL")?;
        let wcash_rpc = rpc_config("WCASH_RPC", true)?
            .ok_or_else(|| ExplorerError::Config("WCASH_RPC_URL is required".to_owned()))?;
        let zcash_rpcs = ["ZCASH_RPC", "ZCASH_RPC_SECONDARY"]
            .into_iter()
            .filter_map(|prefix| rpc_config(prefix, false).transpose())
            .collect::<Result<Vec<_>>>()?;
        let network = NetworkConfig {
            id: env_or("WCASH_NETWORK", "testnet"),
            display_name: env_or("WCASH_NETWORK_NAME", "Wcash Testnet"),
            symbol: env_or("WCASH_SYMBOL", "TWC"),
            decimals: parse_env("WCASH_DECIMALS", 8)?,
            coinbase_maturity: parse_env("WCASH_COINBASE_MATURITY", 100)?,
            target_spacing_seconds: parse_env("WCASH_TARGET_SPACING_SECONDS", 75)?,
            max_supply_zat: parse_env("WCASH_MAX_SUPPLY_ZAT", 2_100_000_000_000_000_i64)?,
            initial_subsidy_zat: parse_env("WCASH_INITIAL_SUBSIDY_ZAT", 625_000_000_i64)?,
            halving_interval: parse_env("WCASH_HALVING_INTERVAL", 1_680_000_u64)?,
            first_halving_height: parse_env("WCASH_FIRST_HALVING_HEIGHT", 1_680_001_u64)?,
            genesis_hash: env_or(
                "WCASH_GENESIS_HASH",
                "0271b5b0a10b2838f43cccdec9ca2f72aa72a7c103830082bac8f82f47f0593a",
            ),
            parent_explorer_block_url: env_or(
                "PARENT_EXPLORER_BLOCK_URL",
                "https://testnet.cipherscan.app/block/{hash}",
            ),
            parent_explorer_tx_url: env_or(
                "PARENT_EXPLORER_TX_URL",
                "https://testnet.cipherscan.app/tx/{txid}",
            ),
        };
        validate_hex_id("WCASH_GENESIS_HASH", &network.genesis_hash)?;
        if network.decimals > 18 {
            return Err(ExplorerError::Config(
                "WCASH_DECIMALS must be at most 18".to_owned(),
            ));
        }
        if network.max_supply_zat <= 0
            || network.initial_subsidy_zat <= 0
            || network.halving_interval == 0
            || network.first_halving_height == 0
        {
            return Err(ExplorerError::Config(
                "supply and halving settings must be positive".to_owned(),
            ));
        }

        let expected_max_supply = i128::from(network.initial_subsidy_zat)
            .checked_mul(i128::from(network.halving_interval))
            .and_then(|value| value.checked_mul(2))
            .ok_or_else(|| ExplorerError::Config("emission schedule overflow".to_owned()))?;
        if network.decimals != 8
            || network.target_spacing_seconds != 75
            || network.initial_subsidy_zat != 625_000_000
            || network.max_supply_zat != 2_100_000_000_000_000
            || i128::from(network.max_supply_zat) != expected_max_supply
            || network.first_halving_height != network.halving_interval.saturating_add(1)
        {
            return Err(ExplorerError::Config(
                "Wcash settings must encode 8 decimals, 75-second blocks, a 6.25 WEC initial subsidy, four-year halvings, and a 21 million WEC cap".to_owned(),
            ));
        }

        let run_indexer = parse_bool("RUN_INDEXER", true)?;
        let require_parent_quorum = parse_bool("REQUIRE_PARENT_QUORUM", true)?;
        let unique_parent_urls = zcash_rpcs
            .iter()
            .map(|rpc| rpc.url.as_str())
            .collect::<HashSet<_>>();
        if unique_parent_urls.len() != zcash_rpcs.len() {
            return Err(ExplorerError::Config(
                "configured Zcash parent RPC endpoints must be distinct".to_owned(),
            ));
        }
        if run_indexer && require_parent_quorum && zcash_rpcs.len() < 2 {
            return Err(ExplorerError::Config(
                "RUN_INDEXER requires two distinct Zcash Testnet RPC endpoints when REQUIRE_PARENT_QUORUM is enabled".to_owned(),
            ));
        }
        let parent_genesis_hash = env_or(
            "ZCASH_PARENT_GENESIS_HASH",
            "05a60a92d99d85997cce3b87616c089f6124d7342af37106edc76126334a2c38",
        );
        validate_hex_id("ZCASH_PARENT_GENESIS_HASH", &parent_genesis_hash)?;

        Ok(Self {
            bind,
            database_url,
            wcash_rpc,
            zcash_rpcs,
            network,
            poll_interval: Duration::from_millis(parse_env("INDEXER_POLL_INTERVAL_MS", 2_000_u64)?),
            rpc_timeout: Duration::from_millis(parse_env("RPC_TIMEOUT_MS", 15_000_u64)?),
            max_reorg_depth: parse_env("MAX_REORG_DEPTH", 10_000_u64)?,
            evidence_refresh_depth: parse_env("EVIDENCE_REFRESH_DEPTH", 100_u64)?,
            migrate: parse_bool("RUN_MIGRATIONS", true)?,
            run_indexer,
            parent_chain: env_or("ZCASH_PARENT_CHAIN", "test"),
            parent_genesis_hash,
            require_parent_quorum,
        })
    }
}

impl std::fmt::Debug for RpcAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => formatter.write_str("None"),
            Self::Basic { username, .. } => formatter
                .debug_struct("Basic")
                .field("username", username)
                .field("password", &"[REDACTED]")
                .finish(),
            Self::Cookie(path) => formatter.debug_tuple("Cookie").field(path).finish(),
        }
    }
}

fn rpc_config(prefix: &str, required_endpoint: bool) -> Result<Option<RpcConfig>> {
    let url_name = format!("{prefix}_URL");
    let Ok(raw_url) = env::var(&url_name) else {
        if required_endpoint {
            return Err(ExplorerError::Config(format!("{url_name} is required")));
        }
        return Ok(None);
    };
    let url = Url::parse(&raw_url)
        .map_err(|error| ExplorerError::Config(format!("invalid {url_name}: {error}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ExplorerError::Config(format!(
            "{url_name} must use http or https"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ExplorerError::Config(format!(
            "credentials must not be embedded in {url_name}"
        )));
    }

    let cookie_name = format!("{prefix}_COOKIE_FILE");
    let username_name = format!("{prefix}_USERNAME");
    let password_name = format!("{prefix}_PASSWORD");
    let auth = match (
        env::var(&cookie_name).ok(),
        env::var(&username_name).ok(),
        env::var(&password_name).ok(),
    ) {
        (Some(cookie), None, None) => RpcAuth::Cookie(PathBuf::from(cookie)),
        (None, Some(username), Some(password)) => RpcAuth::Basic { username, password },
        (None, None, None) => RpcAuth::None,
        _ => {
            return Err(ExplorerError::Config(format!(
                "configure either {cookie_name} or both {username_name}/{password_name}"
            )));
        }
    };
    Ok(Some(RpcConfig {
        label: prefix.to_ascii_lowercase().replace('_', "-"),
        url,
        auth,
    }))
}

fn required(name: &str) -> Result<String> {
    env::var(name).map_err(|_| ExplorerError::Config(format!("{name} is required")))
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn parse_env<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| ExplorerError::Config(format!("invalid {name}: {error}"))),
        Err(_) => Ok(default),
    }
}

fn parse_bool(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(ExplorerError::Config(format!(
                "{name} must be true or false"
            ))),
        },
        Err(_) => Ok(default),
    }
}

fn validate_hex_id(name: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ExplorerError::Config(format!(
            "{name} must be a 32-byte hexadecimal identifier"
        )));
    }
    Ok(())
}
