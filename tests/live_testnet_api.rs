use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::Client;
use serde_json::Value;

const BASE_URL_ENV: &str = "WCASH_EXPLORER_TEST_BASE_URL";
const BLOCK_ONE_HASH: &str = "79cdcea38a54f99a59adddf124490a65de499e5677e693caa14dcf24b5f96007";
const BLOCK_ONE_TXID: &str = "3d8e0583e9f030beff3b483dbbaf552894a2a329c340de74dd0ce5cbd24a713d";
const PARENT_BLOCK_HASH: &str = "0000005d48352ca15798f834f835a9b68e14bda9053c98251124725f8b32ad52";
const PARENT_COINBASE_TXID: &str =
    "1a4466dbf0b2e24e7094b8f6a8b372758b1b37249e99a78195f517149e3aeb4b";
const PAYOUT_ADDRESS: &str = "WT8ZbkEWkWb7sU2iniCE7H5KARkUZUkjsFZ";

#[tokio::test]
#[ignore = "requires a running explorer configured by WCASH_EXPLORER_TEST_BASE_URL"]
async fn published_block_one_evidence_is_consistent_across_the_api() {
    if let Err(error) = run_live_api_test().await {
        panic!("live Wcash Testnet API test failed: {error:#}");
    }
}

async fn run_live_api_test() -> Result<()> {
    let base_url = std::env::var(BASE_URL_ENV)
        .with_context(|| format!("{BASE_URL_ENV} must name the running explorer origin"))?;
    let base_url = base_url.trim_end_matches('/');
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;

    let ready = get_json(&client, base_url, "/health/ready").await?;
    ensure!(ready["status"] == "ready");
    let ready_height = json_u64(&ready["indexedHeight"], "ready indexed height")?;
    ensure!(ready_height >= 1);

    let status = get_json(&client, base_url, "/api/v1/status").await?;
    ensure!(status["data"]["network"] == "testnet");
    ensure!(status["data"]["symbol"] == "tWEC");
    ensure!(status["data"]["maxSupply"]["decimal"] == "21000000.00000000");
    ensure!(status["data"]["initialSubsidy"]["decimal"] == "6.25000000");
    ensure!(status["data"]["targetSpacingSeconds"] == 75);
    ensure!(status["data"]["halvingInterval"] == 1_680_000);
    let status_height = json_u64(&status["data"]["indexedHeight"], "status indexed height")?;
    ensure!(status_height >= ready_height);
    ensure!(
        json_u64(&status["data"]["nextHalvingHeight"], "next halving height")?
            == next_halving_height(status_height)
    );
    ensure!(
        amount_zatoshi(&status["data"]["totalIssued"], "total issued")?
            == expected_issued_zatoshi(status_height)
    );

    let block = get_json(&client, base_url, "/api/v1/blocks/1").await?;
    ensure!(block["data"]["height"] == 1);
    ensure!(block["data"]["hash"] == BLOCK_ONE_HASH);
    ensure!(block["data"]["reward"]["decimal"] == "6.25000000");
    ensure!(block["data"]["transactions"][0]["txid"] == BLOCK_ONE_TXID);
    assert_auxpow(&block["data"]["auxpow"])?;

    let auxpow = get_json(&client, base_url, "/api/v1/blocks/1/auxpow").await?;
    assert_auxpow(&auxpow["data"])?;

    let raw = get_json(&client, base_url, "/api/v1/blocks/1/raw").await?;
    ensure!(raw["data"]["blockHash"] == BLOCK_ONE_HASH);
    ensure!(raw["data"]["encoding"] == "hex");
    ensure!(
        raw["data"]["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty())
    );

    let transaction_path = format!("/api/v1/transactions/{BLOCK_ONE_TXID}?block={BLOCK_ONE_HASH}");
    let transaction = get_json(&client, base_url, &transaction_path).await?;
    ensure!(transaction["data"]["txid"] == BLOCK_ONE_TXID);
    ensure!(transaction["data"]["blockHash"] == BLOCK_ONE_HASH);
    ensure!(
        transaction["data"]["authDigest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64)
    );

    let address_path = format!("/api/v1/addresses/{PAYOUT_ADDRESS}");
    let address = get_json(&client, base_url, &address_path).await?;
    ensure!(address["data"]["address"] == PAYOUT_ADDRESS);
    ensure!(address["data"]["addressType"] == "transparent");
    let total_received = amount_zatoshi(&address["data"]["totalReceived"], "total received")?;
    let total_sent = amount_zatoshi(&address["data"]["totalSent"], "total sent")?;
    let unspent = amount_zatoshi(&address["data"]["unspent"], "unspent balance")?;
    let immature = amount_zatoshi(
        &address["data"]["immatureCoinbase"],
        "immature coinbase balance",
    )?;
    let mature = amount_zatoshi(
        &address["data"]["matureCoinbaseMustShield"],
        "mature coinbase balance",
    )?;
    let non_coinbase = amount_zatoshi(
        &address["data"]["nonCoinbaseUnspent"],
        "non-coinbase balance",
    )?;
    ensure!(total_received >= 625_000_000);
    ensure!(total_sent >= 0);
    ensure!(unspent > 0);
    ensure!(total_received - total_sent == unspent);
    ensure!(immature + mature + non_coinbase == unspent);
    ensure!(json_u64(&address["data"]["utxoCount"], "address UTXO count")? >= 1);
    ensure!(
        json_u64(
            &address["data"]["minedTransactionCount"],
            "mined transaction count"
        )? >= 1
    );
    ensure!(
        json_u64(
            &address["data"]["transactionCount"],
            "address transaction count"
        )? >= 1
    );
    ensure!(address["data"]["firstSeenHeight"] == 1);
    ensure!(address["data"]["canonicalDoubleSpendAnomalies"] == 0);
    let activity = address["data"]["activity"]
        .as_array()
        .context("address activity is missing")?;
    ensure!(!activity.is_empty());
    ensure!(activity.iter().all(|row| {
        row["authDigest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64)
    }));

    let network_history = get_json(&client, base_url, "/api/v1/network/history?limit=2048").await?;
    let network_height = json_u64(
        &network_history["data"]["asOfHeight"],
        "network-history height",
    )?;
    let network_points = network_history["data"]["points"]
        .as_array()
        .context("network history is empty")?;
    assert_complete_history(network_points, network_height, 2_048)?;
    ensure!(
        network_points
            .last()
            .and_then(|point| point["hash"].as_str())
            == network_history["data"]["asOfHash"].as_str()
    );

    let pools = get_json(&client, base_url, "/api/v1/value-pools/history?limit=2048").await?;
    ensure!(pools["data"]["unexpectedPoolSamples"] == 0);
    let pool_height = json_u64(&pools["data"]["asOfHeight"], "value-pool history height")?;
    let pool_points = pools["data"]["points"]
        .as_array()
        .context("value-pool history is missing")?;
    assert_complete_history(pool_points, pool_height, 2_048)?;
    let latest_pools = pool_points.last().context("value-pool history is empty")?;
    ensure!(
        amount_zatoshi(
            &latest_pools["transparent"]["chainValue"],
            "transparent pool value"
        )? >= 0
    );
    ensure!(latest_pools["transparent"]["monitored"] == true);
    let ironwood_monitored = latest_pools["ironwood"]["monitored"]
        .as_bool()
        .context("Ironwood monitoring state is not a boolean")?;
    if ironwood_monitored {
        ensure!(
            amount_zatoshi(
                &latest_pools["ironwood"]["chainValue"],
                "Ironwood pool value"
            )? >= 0
        );
    }

    let merged = get_json(&client, base_url, "/api/v1/merge-mining/stats").await?;
    let merged_height = json_u64(
        &merged["data"]["asOfHeight"],
        "merge-mining statistics height",
    )?;
    let eligible = json_u64(
        &merged["data"]["eligibleChildBlocks"],
        "eligible child blocks",
    )?;
    ensure!(eligible == merged_height);
    for field in [
        "auxpowBlocks",
        "locallyVerifiedBlocks",
        "parentTargetVerifiedBlocks",
        "canonicalParentBlocks",
        "parentQuorumAgreementBlocks",
        "bestChainWitnessBlocks",
        "fullyVerifiedBlocks",
    ] {
        ensure!(
            json_u64(&merged["data"][field], field)? == eligible,
            "unexpected {field}"
        );
    }
    ensure!(merged["data"]["anomalyBlocks"] == 0);
    ensure!(
        merged["data"]["observationSourceCount"]
            .as_i64()
            .is_some_and(|count| count >= 2)
    );

    let rich_list = get_json(&client, base_url, "/api/v1/addresses/rich-list").await?;
    let funded_addresses = json_u64(
        &rich_list["data"]["fundedAddressCount"],
        "funded address count",
    )?;
    let listed_addresses = rich_list["data"]["addresses"]
        .as_array()
        .context("rich-list addresses are missing")?;
    ensure!(funded_addresses >= 1);
    ensure!(funded_addresses >= u64::try_from(listed_addresses.len())?);
    ensure!(!listed_addresses.is_empty());
    ensure!(listed_addresses[0]["rank"] == 1);
    ensure!(amount_zatoshi(&rich_list["data"]["addressedBalance"], "addressed balance")? > 0);
    ensure!(rich_list["data"]["transparentPoolMonitored"] == true);

    let address_stats = get_json(&client, base_url, "/api/v1/addresses/stats?limit=2048").await?;
    let address_stats_height = json_u64(
        &address_stats["data"]["asOfHeight"],
        "address-statistics height",
    )?;
    let address_points = address_stats["data"]["points"]
        .as_array()
        .context("address-statistics history is missing")?;
    assert_complete_history(address_points, address_stats_height, 2_048)?;
    let funded = json_u64(
        &address_stats["data"]["fundedAddressCount"],
        "funded address count",
    )?;
    let seen = json_u64(
        &address_stats["data"]["seenAddressCount"],
        "seen address count",
    )?;
    ensure!(funded >= 1);
    ensure!(seen >= funded);
    ensure!(
        amount_zatoshi(
            &address_stats["data"]["addressedBalance"],
            "addressed balance",
        )? > 0
    );

    let search = get_json(&client, base_url, "/api/v1/search?q=1").await?;
    ensure!(search["data"]["kind"] == "block");
    ensure!(search["data"]["route"] == "/block/1");

    let openapi = get_json(&client, base_url, "/api/openapi.json").await?;
    ensure!(openapi["openapi"] == "3.1.0");
    let paths = openapi["paths"]
        .as_object()
        .context("OpenAPI paths are missing")?;
    for path in [
        "/api/v1/blocks/{id}",
        "/api/v1/blocks/{id}/auxpow",
        "/api/v1/transactions/{txid}",
        "/api/v1/addresses/{address}",
        "/api/v1/network/history",
        "/api/v1/value-pools/history",
        "/api/v1/merge-mining/stats",
        "/api/v1/addresses/stats",
        "/api/v1/addresses/rich-list",
    ] {
        ensure!(paths.contains_key(path), "OpenAPI is missing {path}");
    }
    Ok(())
}

fn json_u64(value: &Value, label: &str) -> Result<u64> {
    value
        .as_u64()
        .with_context(|| format!("{label} is not an unsigned integer"))
}

fn amount_zatoshi(value: &Value, label: &str) -> Result<i128> {
    value["zatoshi"]
        .as_str()
        .with_context(|| format!("{label} has no exact zatoshi value"))?
        .parse::<i128>()
        .with_context(|| format!("{label} is not an exact integer"))
}

fn next_halving_height(height: u64) -> u64 {
    const FIRST_HALVING_HEIGHT: u64 = 1_680_001;
    const HALVING_INTERVAL: u64 = 1_680_000;

    if height < FIRST_HALVING_HEIGHT {
        return FIRST_HALVING_HEIGHT;
    }
    FIRST_HALVING_HEIGHT
        + ((height - FIRST_HALVING_HEIGHT) / HALVING_INTERVAL + 1) * HALVING_INTERVAL
}

fn expected_issued_zatoshi(height: u64) -> i128 {
    const HALVING_INTERVAL: u64 = 1_680_000;
    let mut remaining_blocks = height;
    let mut subsidy = 625_000_000_i128;
    let mut issued = 0_i128;

    while remaining_blocks > 0 && subsidy > 0 {
        let era_blocks = remaining_blocks.min(HALVING_INTERVAL);
        issued += i128::from(era_blocks) * subsidy;
        remaining_blocks -= era_blocks;
        subsidy /= 2;
    }
    issued
}

fn assert_complete_history(points: &[Value], as_of_height: u64, limit: usize) -> Result<()> {
    let expected_len = usize::try_from(as_of_height.saturating_add(1))?.min(limit);
    ensure!(
        points.len() == expected_len,
        "history contains {} point(s), expected {expected_len}",
        points.len()
    );
    let first = points.first().context("canonical history is empty")?;
    let last = points.last().context("canonical history is empty")?;
    ensure!(
        json_u64(&first["height"], "first history height")?
            == as_of_height.saturating_add(1) - u64::try_from(expected_len)?
    );
    ensure!(json_u64(&last["height"], "last history height")? == as_of_height);
    Ok(())
}

fn assert_auxpow(auxpow: &Value) -> Result<()> {
    ensure!(auxpow["localValidationState"] == "auxpow_verified");
    ensure!(auxpow["exactWitnessState"] == "best_chain");
    ensure!(auxpow["parentHashMeetsClaimedTarget"] == true);
    ensure!(auxpow["parentBlockHash"] == PARENT_BLOCK_HASH);
    ensure!(auxpow["parentCoinbaseTxid"] == PARENT_COINBASE_TXID);
    ensure!(auxpow["parentLookupState"] == "canonical");
    ensure!(auxpow["parentSourcesAgree"] == true);
    let observations = auxpow["observations"]
        .as_array()
        .context("AuxPoW observations are missing")?;
    ensure!(observations.len() >= 2);
    ensure!(observations.iter().all(|observation| {
        observation["observationState"] == "canonical"
            && observation["embeddedHeaderMatches"] == true
    }));
    Ok(())
}

async fn get_json(client: &Client, base_url: &str, path: &str) -> Result<Value> {
    let response = client
        .get(format!("{base_url}{path}"))
        .send()
        .await
        .with_context(|| format!("request {path}"))?
        .error_for_status()
        .with_context(|| format!("explorer rejected {path}"))?;
    response
        .json()
        .await
        .with_context(|| format!("decode JSON from {path}"))
}
