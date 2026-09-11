use anyhow::{Context, Result, ensure};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;
use wcashexplorer::{config::NetworkConfig, db::Database, error::ExplorerError};

const ADMIN_DATABASE_URL_ENV: &str = "WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL";

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server configured by WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL"]
async fn presentation_ticker_can_change_without_weakening_network_identity() {
    if let Err(error) = run_network_identity_test().await {
        panic!("PostgreSQL network identity integration test failed: {error:#}");
    }
}

async fn run_network_identity_test() -> Result<()> {
    let admin_database_url = std::env::var(ADMIN_DATABASE_URL_ENV).with_context(|| {
        format!("{ADMIN_DATABASE_URL_ENV} must name a PostgreSQL admin database")
    })?;
    let database_name = format!("wcashexplorer_network_{}", Uuid::new_v4().simple());
    let database_url = create_database(&admin_database_url, &database_name).await?;

    let test_result = exercise_network_identity(&database_url).await;
    let cleanup_result = drop_database(&admin_database_url, &database_name).await;
    match (test_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(test_error), Ok(())) => Err(test_error),
        (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
        (Err(test_error), Err(cleanup_error)) => Err(anyhow::anyhow!(
            "{test_error:#}; additionally failed to remove test database: {cleanup_error:#}"
        )),
    }
}

async fn create_database(admin_database_url: &str, database_name: &str) -> Result<String> {
    ensure_safe_database_name(database_name)?;
    let mut admin = PgConnection::connect(admin_database_url)
        .await
        .context("connect to the PostgreSQL admin database")?;
    sqlx::query(&format!(r#"CREATE DATABASE "{database_name}""#))
        .execute(&mut admin)
        .await
        .context("create the isolated network identity test database")?;

    let mut database_url = url::Url::parse(admin_database_url)
        .context("parse WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL")?;
    database_url.set_path(&format!("/{database_name}"));
    Ok(database_url.to_string())
}

async fn drop_database(admin_database_url: &str, database_name: &str) -> Result<()> {
    ensure_safe_database_name(database_name)?;
    let mut admin = PgConnection::connect(admin_database_url)
        .await
        .context("reconnect to the PostgreSQL admin database for cleanup")?;
    sqlx::query(&format!(r#"DROP DATABASE "{database_name}" WITH (FORCE)"#))
        .execute(&mut admin)
        .await
        .context("drop the isolated network identity test database")?;
    Ok(())
}

fn ensure_safe_database_name(database_name: &str) -> Result<()> {
    ensure!(
        database_name.starts_with("wcashexplorer_network_")
            && database_name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "refusing to create or drop an unexpected database name"
    );
    Ok(())
}

async fn exercise_network_identity(database_url: &str) -> Result<()> {
    let database = Database::connect(database_url).await?;
    database.migrate().await?;

    let mut old_runtime = test_network();
    "tWEC".clone_into(&mut old_runtime.symbol);
    database.initialize_network(&old_runtime).await?;
    ensure!(stored_symbol(&database).await? == "tWEC");

    let new_runtime = test_network();
    database.initialize_network(&new_runtime).await?;
    ensure!(
        stored_symbol(&database).await? == "TWC",
        "the runtime ticker was not persisted over the legacy ticker"
    );

    for (field, mut incompatible_runtime) in incompatible_networks(&new_runtime) {
        incompatible_runtime.symbol = format!("invalid-{field}");
        let error = database
            .initialize_network(&incompatible_runtime)
            .await
            .expect_err("an immutable network mismatch unexpectedly succeeded");
        ensure!(
            matches!(&error, ExplorerError::Config(_)),
            "{field} mismatch returned an unexpected error: {error}"
        );
        ensure!(
            stored_symbol(&database).await? == "TWC",
            "{field} mismatch changed presentation metadata before failing"
        );
    }

    database.pool().close().await;
    Ok(())
}

async fn stored_symbol(database: &Database) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT symbol FROM networks WHERE network_id = 'testnet'")
            .fetch_one(database.pool())
            .await?,
    )
}

fn incompatible_networks(network: &NetworkConfig) -> Vec<(&'static str, NetworkConfig)> {
    let mut variants = Vec::new();

    let mut changed = network.clone();
    "Another network".clone_into(&mut changed.display_name);
    variants.push(("display_name", changed));

    let mut changed = network.clone();
    changed.decimals += 1;
    variants.push(("decimals", changed));

    let mut changed = network.clone();
    changed.coinbase_maturity += 1;
    variants.push(("coinbase_maturity", changed));

    let mut changed = network.clone();
    changed.target_spacing_seconds += 1;
    variants.push(("target_spacing_seconds", changed));

    let mut changed = network.clone();
    changed.max_supply_zat += 1;
    variants.push(("max_supply_zat", changed));

    let mut changed = network.clone();
    changed.initial_subsidy_zat += 1;
    variants.push(("initial_subsidy_zat", changed));

    let mut changed = network.clone();
    changed.halving_interval += 1;
    variants.push(("halving_interval", changed));

    let mut changed = network.clone();
    changed.first_halving_height += 1;
    variants.push(("first_halving_height", changed));

    let mut changed = network.clone();
    changed.genesis_hash = "11".repeat(32);
    variants.push(("genesis_hash", changed));

    variants
}

fn test_network() -> NetworkConfig {
    NetworkConfig {
        id: "testnet".to_owned(),
        display_name: "Wcash Testnet".to_owned(),
        symbol: "TWC".to_owned(),
        decimals: 8,
        coinbase_maturity: 100,
        target_spacing_seconds: 75,
        max_supply_zat: 2_100_000_000_000_000,
        initial_subsidy_zat: 625_000_000,
        halving_interval: 1_680_000,
        first_halving_height: 1_680_001,
        genesis_hash: "00".repeat(32),
        parent_explorer_block_url: "https://example.invalid/block/{hash}".to_owned(),
        parent_explorer_tx_url: "https://example.invalid/tx/{txid}".to_owned(),
    }
}
