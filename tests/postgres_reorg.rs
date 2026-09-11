use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde_json::{Map, json};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;
use wcashexplorer::{
    config::NetworkConfig,
    db::Database,
    models::{ChainValue, IndexedBlock, RpcBlock},
};

const ADMIN_DATABASE_URL_ENV: &str = "WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL";

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server configured by WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL"]
async fn canonical_replacement_is_atomic_and_auditable() {
    if let Err(error) = run_reorg_test().await {
        panic!("PostgreSQL reorg integration test failed: {error:#}");
    }
}

async fn run_reorg_test() -> Result<()> {
    let admin_database_url = std::env::var(ADMIN_DATABASE_URL_ENV).with_context(|| {
        format!("{ADMIN_DATABASE_URL_ENV} must name a PostgreSQL admin database")
    })?;
    let database_name = format!("wcashexplorer_reorg_{}", Uuid::new_v4().simple());
    let database_url = create_database(&admin_database_url, &database_name).await?;

    let test_result = exercise_reorg(&database_url).await;
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
        .context("create the isolated reorg test database")?;

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
        .context("drop the isolated reorg test database")?;
    Ok(())
}

fn ensure_safe_database_name(database_name: &str) -> Result<()> {
    ensure!(
        database_name.starts_with("wcashexplorer_reorg_")
            && database_name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "refusing to create or drop an unexpected database name"
    );
    Ok(())
}

async fn exercise_reorg(database_url: &str) -> Result<()> {
    let database = Database::connect(database_url).await?;
    database.migrate().await?;
    database.initialize_network(&test_network()).await?;

    let old_genesis = synthetic_block("00", 0, None, 100);
    let old_one = synthetic_block("11", 1, Some(old_genesis.block.hash.clone()), 101);
    let old_two = synthetic_block("22", 2, Some(old_one.block.hash.clone()), 102);
    database.commit_block("testnet", &old_genesis).await?;
    database.commit_block("testnet", &old_one).await?;
    database.commit_block("testnet", &old_two).await?;

    let old_tip = database
        .canonical_tip("testnet")
        .await?
        .context("canonical old tip is missing")?;
    ensure!(old_tip.height == 2 && old_tip.block_hash == old_two.block.hash);

    let new_two = synthetic_block("33", 2, Some(old_one.block.hash.clone()), 202);
    let new_three = synthetic_block("44", 3, Some(new_two.block.hash.clone()), 203);

    // Model a previously observed orphan. Reusing its block hash with different
    // immutable metadata makes the second staged insert fail inside the reorg.
    database
        .refresh_block_evidence("testnet", &new_three)
        .await?;
    let mut conflicting_three = new_three.clone();
    conflicting_three.block.size += 1;

    let failure = database
        .replace_canonical_branch(
            "testnet",
            &old_tip,
            Some((1, old_one.block.hash.clone())),
            &[new_two.clone(), conflicting_three],
        )
        .await;
    ensure!(
        failure.is_err(),
        "conflicting replacement unexpectedly succeeded"
    );
    let failure = failure.expect_err("error checked above").to_string();
    ensure!(
        failure.contains("immutable block facts changed"),
        "replacement failed for an unexpected reason: {failure}"
    );

    let tip_after_failure = database
        .canonical_tip("testnet")
        .await?
        .context("tip disappeared after failed replacement")?;
    ensure!(
        tip_after_failure == old_tip,
        "failed replacement changed the canonical tip"
    );
    let staged_new_two_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM blocks WHERE block_hash = $1")
            .bind(&new_two.block.hash)
            .fetch_one(database.pool())
            .await?;
    ensure!(
        staged_new_two_count == 0,
        "the first staged block escaped the failed transaction"
    );
    let event_count_after_failure: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reorg_events")
        .fetch_one(database.pool())
        .await?;
    ensure!(
        event_count_after_failure == 0,
        "a failed replacement published a reorg event"
    );

    database
        .replace_canonical_branch(
            "testnet",
            &old_tip,
            Some((1, old_one.block.hash.clone())),
            &[new_two.clone(), new_three.clone()],
        )
        .await?;

    let new_tip = database
        .canonical_tip("testnet")
        .await?
        .context("new canonical tip is missing")?;
    ensure!(new_tip.height == 3 && new_tip.block_hash == new_three.block.hash);
    let canonical_hashes: Vec<(i64, String)> = sqlx::query_as(
        "SELECT height, block_hash FROM canonical_chain
         WHERE network_id = 'testnet' ORDER BY height",
    )
    .fetch_all(database.pool())
    .await?;
    ensure!(
        canonical_hashes
            == vec![
                (0, old_genesis.block.hash.clone()),
                (1, old_one.block.hash.clone()),
                (2, new_two.block.hash.clone()),
                (3, new_three.block.hash.clone()),
            ],
        "successful replacement was not published as one contiguous branch"
    );

    let old_two_is_detached: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM blocks WHERE block_hash = $1)
         AND NOT EXISTS(SELECT 1 FROM canonical_chain WHERE block_hash = $1)",
    )
    .bind(&old_two.block.hash)
    .fetch_one(database.pool())
    .await?;
    ensure!(
        old_two_is_detached,
        "replaced block was not retained as detached audit history"
    );
    let event: (i64, String, Option<i64>, Option<String>, bool) = sqlx::query_as(
        "SELECT old_tip_height, old_tip_hash, common_ancestor_height,
                common_ancestor_hash, completed_at IS NOT NULL
         FROM reorg_events",
    )
    .fetch_one(database.pool())
    .await?;
    ensure!(
        event
            == (
                2,
                old_two.block.hash,
                Some(1),
                Some(old_one.block.hash),
                true,
            ),
        "completed reorg audit record is incorrect"
    );

    database.pool().close().await;
    Ok(())
}

fn synthetic_block(
    hash_byte: &str,
    height: u64,
    previousblockhash: Option<String>,
    size: u64,
) -> IndexedBlock {
    let hash = hash_byte.repeat(32);
    IndexedBlock {
        block: RpcBlock {
            hash: hash.clone(),
            height,
            confirmations: 1,
            size,
            time: 1_789_000_000 + i64::try_from(height).expect("small test height"),
            bits: "1e008859".to_owned(),
            difficulty: json!(1.0),
            nonce: "00".repeat(32),
            solution: String::new(),
            merkleroot: "55".repeat(32),
            blockcommitments: Some("66".repeat(32)),
            previousblockhash,
            nextblockhash: None,
            tx: Vec::new(),
            chain_supply: Some(ChainValue {
                chain_value_zat: Some(
                    i64::try_from(height).expect("small test height") * 625_000_000,
                ),
                monitored: Some(true),
            }),
            value_pools: Vec::new(),
            extra: Map::new(),
        },
        auxpow: None,
        fetched_at: Utc::now(),
        raw_block: hash.into_bytes(),
        raw: json!({"fixture": "postgres-reorg"}),
    }
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
