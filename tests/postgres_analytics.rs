use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use sqlx::{Connection, PgConnection};
use tower::ServiceExt;
use uuid::Uuid;
use wcashexplorer::{
    api::{self, AppState},
    config::NetworkConfig,
    db::Database,
    models::{
        ChainValue, IndexedBlock, ParentBlock, ParentLookupState, ParentObservation, RpcBlock,
        RpcInput, RpcOutput, RpcTransaction, ScriptPubKey, ValuePool, VerifiedAuxPow,
    },
};
use zebra_chain::{
    parameters::{Network, NetworkKind},
    transparent::Address as TransparentAddress,
};

const ADMIN_DATABASE_URL_ENV: &str = "WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL";
const COIN: i64 = 100_000_000;

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server configured by WCASH_EXPLORER_TEST_ADMIN_DATABASE_URL"]
async fn canonical_analytics_exclude_a_replaced_branch() {
    if let Err(error) = run_analytics_test().await {
        panic!("PostgreSQL analytics integration test failed: {error:#}");
    }
}

async fn run_analytics_test() -> Result<()> {
    let admin_database_url = std::env::var(ADMIN_DATABASE_URL_ENV).with_context(|| {
        format!("{ADMIN_DATABASE_URL_ENV} must name a PostgreSQL admin database")
    })?;
    let database_name = format!("wcashexplorer_analytics_{}", Uuid::new_v4().simple());
    let database_url = create_database(&admin_database_url, &database_name).await?;

    let test_result = exercise_analytics(&database_url).await;
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
        .context("create the isolated analytics test database")?;

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
        .context("drop the isolated analytics test database")?;
    Ok(())
}

fn ensure_safe_database_name(database_name: &str) -> Result<()> {
    ensure!(
        database_name.starts_with("wcashexplorer_analytics_")
            && database_name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "refusing to create or drop an unexpected database name"
    );
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn exercise_analytics(database_url: &str) -> Result<()> {
    let parent_network = Network::new_wcash_testnet();
    let address_a = TransparentAddress::from_pub_key_hash(NetworkKind::Testnet, [0x0a; 20])
        .encode_wcash(&parent_network)
        .context("encode fixture address A")?;
    let address_b = TransparentAddress::from_pub_key_hash(NetworkKind::Testnet, [0x0b; 20])
        .encode_wcash(&parent_network)
        .context("encode fixture address B")?;
    ensure!(address_a.starts_with("WT") && address_b.starts_with("WT"));
    ensure!(address_a != address_b);

    let genesis_hash = identifier(0x10);
    let network = test_network(genesis_hash.clone());
    let database = Database::connect(database_url).await?;
    database.migrate().await?;
    database.initialize_network(&network).await?;

    let mut genesis_tx = coinbase_transaction(0x40, 100 * COIN, &address_b);
    genesis_tx.version = 1;
    genesis_tx.authdigest = Some("ff".repeat(32));
    let genesis = synthetic_block(
        0x10,
        0,
        None,
        vec![genesis_tx.clone()],
        100 * COIN,
        100 * COIN,
    );

    let block_one_coinbase = coinbase_transaction(0x41, 10 * COIN, &address_a);
    let incoming = transparent_transaction(
        0x50,
        &genesis_tx.txid,
        0,
        vec![
            (60 * COIN, address_a.clone()),
            (40 * COIN, address_b.clone()),
        ],
    );
    let block_one = synthetic_block(
        0x11,
        1,
        Some(genesis.block.hash.clone()),
        vec![block_one_coinbase.clone(), incoming.clone()],
        110 * COIN,
        10 * COIN,
    );

    let old_coinbase = coinbase_transaction(0x42, 5 * COIN, &address_a);
    let detached_spend = transparent_transaction(
        0x51,
        &incoming.txid,
        1,
        vec![(39 * COIN, address_a.clone()), (COIN, address_b.clone())],
    );
    let mut old_two = synthetic_block(
        0x12,
        2,
        Some(block_one.block.hash.clone()),
        vec![old_coinbase, detached_spend.clone()],
        115 * COIN,
        5 * COIN,
    );
    // A legacy pool on the detached branch must not become a canonical anomaly.
    old_two.block.value_pools.push(ValuePool {
        id: "legacy-fixture".to_owned(),
        chain_value_zat: Some(COIN),
        value_delta_zat: Some(COIN),
        monitored: Some(true),
    });

    database.commit_block("testnet", &genesis).await?;
    let mut metadata_refresh = genesis.clone();
    metadata_refresh.block.tx[0]
        .extra
        .insert("confirmations".to_owned(), json!(99));
    database
        .refresh_block_evidence("testnet", &metadata_refresh)
        .await
        .context("metadata-only transaction refresh must remain valid")?;
    let mut altered_genesis = genesis.clone();
    altered_genesis.block.tx[0].vout[0].value_zat += 1;
    ensure!(
        database
            .refresh_block_evidence("testnet", &altered_genesis)
            .await
            .is_err(),
        "a repeated transaction instance with changed outputs was accepted"
    );
    database.commit_block("testnet", &block_one).await?;
    let mut missing_modern_auth_digest = block_one.clone();
    missing_modern_auth_digest.block.tx[0].authdigest = None;
    ensure!(
        database
            .refresh_block_evidence("testnet", &missing_modern_auth_digest)
            .await
            .is_err(),
        "a version 6 transaction without an authorization digest was accepted"
    );
    let mut altered_input = block_one.clone();
    altered_input.block.tx[1].vin[0].sequence = Some(1);
    ensure!(
        database
            .refresh_block_evidence("testnet", &altered_input)
            .await
            .is_err(),
        "a repeated transaction instance with changed inputs was accepted"
    );
    let mut altered_shielded_count = block_one.clone();
    altered_shielded_count.block.tx[1]
        .shielded_spends
        .push(json!({"fixture": true}));
    ensure!(
        database
            .refresh_block_evidence("testnet", &altered_shielded_count)
            .await
            .is_err(),
        "a repeated transaction instance with changed shielded structure was accepted"
    );
    let mut altered_position = block_one.clone();
    altered_position.block.tx.swap(0, 1);
    ensure!(
        database
            .refresh_block_evidence("testnet", &altered_position)
            .await
            .is_err(),
        "a repeated block with changed transaction positions was accepted"
    );
    database.commit_block("testnet", &old_two).await?;
    let old_tip = database
        .canonical_tip("testnet")
        .await?
        .context("old canonical tip is missing")?;
    ensure!(old_tip.height == 2 && old_tip.block_hash == old_two.block.hash);

    let new_coinbase = coinbase_transaction(0x43, 5 * COIN, &address_a);
    let self_spend = transparent_transaction(
        0x52,
        &incoming.txid,
        0,
        vec![
            (20 * COIN, address_a.clone()),
            (40 * COIN, address_b.clone()),
        ],
    );
    let new_two = synthetic_block(
        0x13,
        2,
        Some(block_one.block.hash.clone()),
        vec![new_coinbase, self_spend.clone()],
        115 * COIN,
        5 * COIN,
    );
    database
        .replace_canonical_branch(
            "testnet",
            &old_tip,
            Some((1, block_one.block.hash.clone())),
            std::slice::from_ref(&new_two),
        )
        .await?;
    database
        .update_node_tip("testnet", 2, &new_two.block.hash, "ready", None)
        .await?;

    let app = api::router(AppState::new(database.clone(), network.clone(), false));

    let genesis_view = get_json(&app, "/api/v1/blocks/0").await?;
    let indexed_genesis_tx = &genesis_view["data"]["transactions"][0];
    ensure!(indexed_genesis_tx["authDigest"].is_null());
    ensure!(indexed_genesis_tx["instanceDigestKind"] == "explorer-raw-hash");
    ensure!(
        indexed_genesis_tx["instanceDigest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64)
    );

    let ready = get_json(&app, "/health/ready").await?;
    ensure!(ready["status"] == "ready");
    ensure!(ready["indexedHeight"] == 2);

    let refresh_candidate = database
        .evidence_refresh_candidate("testnet", 100)
        .await?
        .context("freshness scheduler did not select an AuxPoW block")?;
    ensure!(
        refresh_candidate == (2, new_two.block.hash.clone()),
        "the canonical tip must be refreshed before older evidence"
    );

    sqlx::query(
        "UPDATE auxpow_links SET verified_at = now()
         WHERE block_hash = $1 AND witness_hash = $2",
    )
    .bind(&new_two.block.hash)
    .bind(
        &new_two
            .auxpow
            .as_ref()
            .expect("fixture AuxPoW")
            .witness_hash,
    )
    .execute(database.pool())
    .await?;
    sqlx::query(
        "UPDATE parent_chain_observations SET checked_at = now()
         WHERE block_hash = $1 AND witness_hash = $2",
    )
    .bind(&new_two.block.hash)
    .bind(
        &new_two
            .auxpow
            .as_ref()
            .expect("fixture AuxPoW")
            .witness_hash,
    )
    .execute(database.pool())
    .await?;
    let strict_app = api::router(AppState::new(database.clone(), network.clone(), true));
    let strict_ready = get_json(&strict_app, "/health/ready").await?;
    ensure!(strict_ready["status"] == "ready");

    sqlx::query(
        "UPDATE parent_chain_observations SET observation_state = 'unavailable'
         WHERE block_hash = $1 AND witness_hash = $2 AND source_name = 'parent-b'",
    )
    .bind(&new_two.block.hash)
    .bind(
        &new_two
            .auxpow
            .as_ref()
            .expect("fixture AuxPoW")
            .witness_hash,
    )
    .execute(database.pool())
    .await?;
    assert_not_ready(&strict_app, "/health/ready").await?;
    sqlx::query(
        "UPDATE parent_chain_observations
         SET observation_state = 'canonical', embedded_header_matches = TRUE,
             checked_at = now() - interval '61 seconds'
         WHERE block_hash = $1 AND witness_hash = $2",
    )
    .bind(&new_two.block.hash)
    .bind(
        &new_two
            .auxpow
            .as_ref()
            .expect("fixture AuxPoW")
            .witness_hash,
    )
    .execute(database.pool())
    .await?;
    assert_not_ready(&strict_app, "/health/ready").await?;
    sqlx::query(
        "UPDATE parent_chain_observations SET checked_at = now()
         WHERE block_hash = $1 AND witness_hash = $2",
    )
    .bind(&new_two.block.hash)
    .bind(
        &new_two
            .auxpow
            .as_ref()
            .expect("fixture AuxPoW")
            .witness_hash,
    )
    .execute(database.pool())
    .await?;
    get_json(&strict_app, "/health/ready").await?;

    let block_one_witness = &block_one
        .auxpow
        .as_ref()
        .expect("fixture AuxPoW")
        .witness_hash;
    sqlx::query(
        "UPDATE block_witnesses SET witness_confirmations = 999
         WHERE block_hash = $1 AND witness_hash = $2",
    )
    .bind(&block_one.block.hash)
    .bind(block_one_witness)
    .execute(database.pool())
    .await?;
    let block_one_auxpow = get_json(
        &strict_app,
        &format!("/api/v1/blocks/{}/auxpow", block_one.block.hash),
    )
    .await?;
    ensure!(
        block_one_auxpow["data"]["witnessConfirmations"] == 2,
        "AuxPoW confirmations must be derived from the current canonical tip"
    );

    let address = get_json(&app, &format!("/api/v1/addresses/{address_a}")).await?;
    let address = &address["data"];
    ensure!(address["totalReceived"]["decimal"] == "95.00000000");
    ensure!(address["totalSent"]["decimal"] == "60.00000000");
    ensure!(address["unspent"]["decimal"] == "35.00000000");
    ensure!(address["immatureCoinbase"]["decimal"] == "15.00000000");
    ensure!(address["matureCoinbaseMustShield"]["decimal"] == "0.00000000");
    ensure!(address["nonCoinbaseUnspent"]["decimal"] == "20.00000000");
    ensure!(address["utxoCount"] == 3);
    ensure!(address["minedOutputCount"] == 2);
    ensure!(address["minedTransactionCount"] == 2);
    ensure!(address["transactionCount"] == 4);
    ensure!(address["firstSeenHeight"] == 1);
    ensure!(address["lastSeenHeight"] == 2);
    ensure!(address["canonicalDoubleSpendAnomalies"] == 0);
    let latest_activity = address["activity"]
        .as_array()
        .and_then(|rows| rows.first())
        .context("address activity is empty")?;
    ensure!(latest_activity["txid"] == self_spend.txid);
    ensure!(latest_activity["direction"] == "self");
    ensure!(latest_activity["received"]["decimal"] == "20.00000000");
    ensure!(latest_activity["sent"]["decimal"] == "60.00000000");
    ensure!(latest_activity["net"]["decimal"] == "-40.00000000");
    ensure!(latest_activity["balanceAfter"]["decimal"] == "35.00000000");

    let rich_list = get_json(&app, "/api/v1/addresses/rich-list?limit=10").await?;
    let rich_list = &rich_list["data"];
    ensure!(rich_list["asOfHeight"] == 2);
    ensure!(rich_list["asOfHash"] == new_two.block.hash);
    ensure!(rich_list["transparentPool"]["decimal"] == "115.00000000");
    ensure!(rich_list["transparentPoolReported"] == true);
    ensure!(rich_list["transparentPoolMonitored"] == true);
    ensure!(rich_list["fundedAddressCount"] == 2);
    ensure!(rich_list["addressedBalance"]["decimal"] == "115.00000000");
    ensure!(rich_list["addresslessOrUndecodedBalance"]["decimal"] == "0.00000000");
    ensure!(rich_list["top1Balance"]["decimal"] == "80.00000000");
    ensure!(rich_list["top10Balance"]["decimal"] == "115.00000000");
    let ranked = rich_list["addresses"]
        .as_array()
        .context("rich list is not an array")?;
    ensure!(ranked.len() == 2);
    ensure!(ranked[0]["address"] == address_b);
    ensure!(ranked[0]["balance"]["decimal"] == "80.00000000");
    ensure!(ranked[1]["address"] == address_a);
    ensure!(ranked[1]["balance"]["decimal"] == "35.00000000");
    ensure!(ranked[1]["totalReceived"]["decimal"] == "95.00000000");
    ensure!(ranked[1]["totalSent"]["decimal"] == "60.00000000");

    let stats = get_json(&app, "/api/v1/addresses/stats?limit=10").await?;
    let stats = &stats["data"];
    ensure!(stats["asOfHeight"] == 2);
    ensure!(stats["fundedAddressCount"] == 2);
    ensure!(stats["seenAddressCount"] == 2);
    ensure!(stats["addressedBalance"]["decimal"] == "115.00000000");
    ensure!(stats["transparentPool"]["decimal"] == "115.00000000");
    ensure!(stats["transparentPoolReported"] == true);
    ensure!(stats["addresslessOrUndecodedBalance"]["decimal"] == "0.00000000");
    let address_points = stats["points"]
        .as_array()
        .context("address history is not an array")?;
    ensure!(address_points.len() == 3);
    ensure!(address_points[0]["height"] == 0);
    ensure!(address_points[0]["activeAddresses"] == 1);
    ensure!(address_points[0]["newAddresses"] == 1);
    ensure!(address_points[1]["height"] == 1);
    ensure!(address_points[1]["activeAddresses"] == 2);
    ensure!(address_points[1]["newAddresses"] == 1);
    ensure!(address_points[2]["height"] == 2);
    ensure!(address_points[2]["activeAddresses"] == 2);
    ensure!(address_points[2]["newAddresses"] == 0);
    ensure!(address_points[2]["totalSeenAddresses"] == 2);

    let history = get_json(&app, "/api/v1/network/history?limit=10").await?;
    let history = &history["data"];
    ensure!(history["asOfHeight"] == 2);
    ensure!(history["asOfHash"] == new_two.block.hash);
    let history_points = history["points"]
        .as_array()
        .context("network history is not an array")?;
    ensure!(history_points.len() == 3);
    ensure!(history_points[0]["height"] == 0);
    ensure!(history_points[0]["spacingSeconds"].is_null());
    ensure!(history_points[1]["spacingSeconds"] == 75);
    ensure!(history_points[2]["hash"] == new_two.block.hash);
    ensure!(history_points[2]["totalIssued"]["decimal"] == "115.00000000");
    let paged_history = get_json(&app, "/api/v1/network/history?before=3&limit=2").await?;
    let paged_points = paged_history["data"]["points"]
        .as_array()
        .context("paged network history is not an array")?;
    ensure!(paged_points.len() == 2);
    ensure!(paged_points[0]["height"] == 1);
    ensure!(paged_points[0]["spacingSeconds"] == 75);
    ensure!(paged_points[1]["height"] == 2);

    let pools = get_json(&app, "/api/v1/value-pools/history?limit=10").await?;
    let pools = &pools["data"];
    ensure!(pools["asOfHeight"] == 2);
    ensure!(pools["unexpectedPoolSamples"] == 0);
    let pool_points = pools["points"]
        .as_array()
        .context("value-pool history is not an array")?;
    ensure!(pool_points.len() == 3);
    let latest_pool = pool_points.last().context("value-pool history is empty")?;
    ensure!(latest_pool["hash"] == new_two.block.hash);
    ensure!(latest_pool["transparent"]["chainValue"]["decimal"] == "115.00000000");
    ensure!(latest_pool["transparent"]["valueDelta"]["decimal"] == "5.00000000");
    ensure!(latest_pool["transparent"]["monitored"] == true);
    ensure!(latest_pool["transparent"]["reported"] == true);
    ensure!(latest_pool["ironwood"]["chainValue"]["decimal"] == "0.00000000");
    ensure!(latest_pool["ironwood"]["reported"] == true);
    ensure!(latest_pool["ironwood"]["monitored"] == false);

    let merge = get_json(&app, "/api/v1/merge-mining/stats").await?;
    let merge = &merge["data"];
    for field in [
        "eligibleChildBlocks",
        "auxpowBlocks",
        "locallyVerifiedBlocks",
        "parentTargetVerifiedBlocks",
        "canonicalParentBlocks",
        "parentQuorumAgreementBlocks",
        "bestChainWitnessBlocks",
        "fullyVerifiedBlocks",
    ] {
        ensure!(
            merge[field] == 2,
            "unexpected merge-mining count for {field}"
        );
    }
    ensure!(merge["observationSourceCount"] == 2);
    ensure!(merge["anomalyBlocks"] == 0);

    let reorgs = get_json(&app, "/api/v1/reorgs").await?;
    let reorg = reorgs["data"]
        .as_array()
        .and_then(|rows| rows.first())
        .context("reorg audit history is empty")?;
    ensure!(reorg["oldTipHeight"] == 2);
    ensure!(reorg["oldTipHash"] == old_two.block.hash);
    ensure!(reorg["commonAncestorHeight"] == 1);
    ensure!(reorg["commonAncestorHash"] == block_one.block.hash);
    ensure!(!reorg["completedAt"].is_null());

    assert_not_found(&app, &format!("/api/v1/search?q={}", old_two.block.hash)).await?;
    assert_not_found(&app, &format!("/api/v1/search?q={}", detached_spend.txid)).await?;
    assert_not_found(&app, &format!("/api/v1/blocks/{}", old_two.block.hash)).await?;
    assert_not_found(
        &app,
        &format!("/api/v1/transactions/{}", detached_spend.txid),
    )
    .await?;
    let canonical_block_search =
        get_json(&app, &format!("/api/v1/search?q={}", new_two.block.hash)).await?;
    ensure!(canonical_block_search["data"]["kind"] == "block");
    let canonical_tx_search =
        get_json(&app, &format!("/api/v1/search?q={}", self_spend.txid)).await?;
    ensure!(canonical_tx_search["data"]["kind"] == "transaction");

    let detached_block_is_retained: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blocks WHERE block_hash = $1)")
            .bind(&old_two.block.hash)
            .fetch_one(database.pool())
            .await?;
    ensure!(
        detached_block_is_retained,
        "replaced block disappeared instead of remaining available for audit"
    );
    let detached_transaction_is_retained: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM transaction_instances WHERE txid = $1)")
            .bind(&detached_spend.txid)
            .fetch_one(database.pool())
            .await?;
    ensure!(
        detached_transaction_is_retained,
        "replaced transaction disappeared instead of remaining available for audit"
    );

    sqlx::query(
        "DELETE FROM value_pool_snapshots
         WHERE block_hash = $1 AND witness_hash = $2 AND pool_id = 'transparent'",
    )
    .bind(&new_two.block.hash)
    .bind(
        &new_two
            .auxpow
            .as_ref()
            .expect("fixture AuxPoW")
            .witness_hash,
    )
    .execute(database.pool())
    .await?;
    let missing_tip_pool = get_json(&app, "/api/v1/addresses/stats?limit=10").await?;
    ensure!(missing_tip_pool["data"]["transparentPool"].is_null());
    ensure!(missing_tip_pool["data"]["transparentPoolReported"] == false);
    ensure!(missing_tip_pool["data"]["transparentPoolMonitored"].is_null());
    ensure!(missing_tip_pool["data"]["addresslessOrUndecodedBalance"].is_null());
    let missing_tip_rich_list = get_json(&app, "/api/v1/addresses/rich-list?limit=10").await?;
    ensure!(missing_tip_rich_list["data"]["transparentPool"].is_null());
    ensure!(missing_tip_rich_list["data"]["transparentPoolReported"] == false);
    ensure!(missing_tip_rich_list["data"]["transparentPoolMonitored"].is_null());
    ensure!(missing_tip_rich_list["data"]["addresslessOrUndecodedBalance"].is_null());

    drop(app);
    database.pool().close().await;
    Ok(())
}

async fn get_json(app: &Router, path: &str) -> Result<Value> {
    let (status, body) = request_json(app, path).await?;
    ensure!(
        status == StatusCode::OK,
        "API rejected {path} with {status}: {body}"
    );
    Ok(body)
}

async fn assert_not_found(app: &Router, path: &str) -> Result<()> {
    let (status, body) = request_json(app, path).await?;
    ensure!(
        status == StatusCode::NOT_FOUND,
        "expected {path} to return 404, got {status}: {body}"
    );
    Ok(())
}

async fn assert_not_ready(app: &Router, path: &str) -> Result<()> {
    let (status, body) = request_json(app, path).await?;
    ensure!(
        status == StatusCode::SERVICE_UNAVAILABLE,
        "expected {path} to return 503, got {status}: {body}"
    );
    ensure!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("parent evidence")),
        "readiness failure did not identify parent evidence: {body}"
    );
    Ok(())
}

async fn request_json(app: &Router, path: &str) -> Result<(StatusCode, Value)> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .context("build API request")?,
        )
        .await
        .expect("the Axum router is infallible");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .context("read API response body")?;
    let body = serde_json::from_slice(&bytes).context("decode API response JSON")?;
    Ok((status, body))
}

fn synthetic_block(
    tag: u8,
    height: u64,
    previousblockhash: Option<String>,
    tx: Vec<RpcTransaction>,
    chain_supply_zat: i64,
    transparent_delta_zat: i64,
) -> IndexedBlock {
    let hash = identifier(tag);
    let block_time = 1_789_000_000 + i64::try_from(height).expect("small fixture height") * 75;
    let auxpow = (height > 0).then(|| verified_auxpow(tag, height, block_time));
    IndexedBlock {
        block: RpcBlock {
            hash: hash.clone(),
            height,
            confirmations: 1,
            size: 1_000 + height,
            time: block_time,
            bits: "1e008859".to_owned(),
            difficulty: json!(1.0),
            nonce: identifier(tag.wrapping_add(0x70)),
            solution: if height > 0 {
                "00".to_owned()
            } else {
                String::new()
            },
            merkleroot: identifier(tag.wrapping_add(0x20)),
            blockcommitments: Some(identifier(tag.wrapping_add(0x30))),
            previousblockhash,
            nextblockhash: None,
            tx,
            chain_supply: Some(ChainValue {
                chain_value_zat: Some(chain_supply_zat),
                monitored: Some(true),
            }),
            value_pools: vec![
                ValuePool {
                    id: "transparent".to_owned(),
                    chain_value_zat: Some(chain_supply_zat),
                    value_delta_zat: Some(transparent_delta_zat),
                    monitored: Some(true),
                },
                ValuePool {
                    id: "ironwood".to_owned(),
                    chain_value_zat: Some(0),
                    value_delta_zat: Some(0),
                    monitored: Some(false),
                },
            ],
            extra: Map::new(),
        },
        auxpow,
        fetched_at: fixture_time(block_time),
        raw_block: vec![tag],
        raw: json!({"fixture": "postgres-analytics", "tag": tag}),
    }
}

fn verified_auxpow(tag: u8, height: u64, block_time: i64) -> VerifiedAuxPow {
    let parent_hash = identifier(tag.wrapping_add(0x80));
    let checked_at = fixture_time(block_time + 1);
    let parent = ParentBlock {
        hash: parent_hash.clone(),
        height: 10_000 + height,
        confirmations: 2,
        time: block_time,
        bits: "1e008859".to_owned(),
        difficulty: json!(1.0),
        tx: Vec::new(),
    };
    VerifiedAuxPow {
        witness_hash: identifier(tag.wrapping_add(0x60)),
        proof_version: 1,
        proof_size: 1,
        parent_block_hash: parent_hash,
        parent_header_bits: "1e008859".to_owned(),
        parent_hash_meets_claimed_target: true,
        parent_coinbase_txid: identifier(tag.wrapping_add(0x90)),
        parent_merkle_depth: 0,
        parent_coinbase_index: 0,
        auth_data_merkle_depth: 0,
        auth_data_coinbase_index: 0,
        auxiliary_merkle_depth: 0,
        auxiliary_index: 0,
        verification_state: "auxpow_verified".to_owned(),
        verifier_version: "postgres-analytics-fixture-v1".to_owned(),
        exact_witness_state: "best_chain".to_owned(),
        witness_confirmations: Some(1),
        parent_observations: ["parent-a", "parent-b"]
            .into_iter()
            .map(|source| ParentObservation {
                source: source.to_owned(),
                state: ParentLookupState::Canonical,
                block: Some(parent.clone()),
                embedded_header_matches: Some(true),
                checked_at,
            })
            .collect(),
        parent_lookup_state: ParentLookupState::Canonical,
        parent_sources_agree: true,
    }
}

fn coinbase_transaction(tag: u8, value_zat: i64, address: &str) -> RpcTransaction {
    transaction(
        tag,
        vec![RpcInput {
            txid: None,
            vout: None,
            coinbase: Some(format!("fixture-{tag:02x}")),
            sequence: Some(u64::from(u32::MAX)),
            script_sig: None,
            extra: Map::new(),
        }],
        vec![(value_zat, address.to_owned())],
    )
}

fn transparent_transaction(
    tag: u8,
    previous_txid: &str,
    previous_output_index: u32,
    outputs: Vec<(i64, String)>,
) -> RpcTransaction {
    transaction(
        tag,
        vec![RpcInput {
            txid: Some(previous_txid.to_owned()),
            vout: Some(previous_output_index),
            coinbase: None,
            sequence: Some(u64::from(u32::MAX)),
            script_sig: None,
            extra: Map::new(),
        }],
        outputs,
    )
}

fn transaction(tag: u8, vin: Vec<RpcInput>, outputs: Vec<(i64, String)>) -> RpcTransaction {
    RpcTransaction {
        txid: identifier(tag),
        authdigest: Some(identifier(tag.wrapping_add(0xa0))),
        version: 6,
        size: 250,
        locktime: 0,
        expiryheight: 0,
        vin,
        vout: outputs
            .into_iter()
            .enumerate()
            .map(|(index, (value_zat, address))| RpcOutput {
                value_zat,
                n: u32::try_from(index).expect("small fixture output index"),
                script_pub_key: ScriptPubKey {
                    asm: None,
                    hex: Some("00".to_owned()),
                    script_type: Some("pubkeyhash".to_owned()),
                    address: Some(address),
                    addresses: Vec::new(),
                },
                extra: Map::new(),
            })
            .collect(),
        shielded_spends: Vec::new(),
        shielded_outputs: Vec::new(),
        orchard: None,
        ironwood: None,
        value_balance_zat: None,
        hex: Some(format!("{tag:02x}")),
        extra: Map::new(),
    }
}

fn fixture_time(timestamp: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(timestamp, 0).expect("valid fixture timestamp")
}

fn identifier(tag: u8) -> String {
    format!("{tag:02x}").repeat(32)
}

fn test_network(genesis_hash: String) -> NetworkConfig {
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
        genesis_hash,
        parent_explorer_block_url: "https://example.invalid/block/{hash}".to_owned(),
        parent_explorer_tx_url: "https://example.invalid/tx/{txid}".to_owned(),
    }
}
