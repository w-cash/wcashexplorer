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
    ensure!(
        ready["indexedHeight"]
            .as_u64()
            .is_some_and(|height| height >= 1)
    );

    let status = get_json(&client, base_url, "/api/v1/status").await?;
    ensure!(status["data"]["network"] == "testnet");
    ensure!(status["data"]["symbol"] == "tWEC");
    ensure!(status["data"]["maxSupply"]["decimal"] == "21000000.00000000");
    ensure!(status["data"]["initialSubsidy"]["decimal"] == "6.25000000");
    ensure!(status["data"]["targetSpacingSeconds"] == 75);
    ensure!(status["data"]["nextHalvingHeight"] == 1_680_001);

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
    ensure!(address["data"]["totalReceived"]["decimal"] == "300.00000000");
    ensure!(address["data"]["totalSent"]["decimal"] == "0.00000000");
    ensure!(address["data"]["unspent"]["decimal"] == "300.00000000");
    ensure!(address["data"]["utxoCount"] == 48);
    ensure!(address["data"]["transactionCount"] == 48);
    ensure!(address["data"]["canonicalDoubleSpendAnomalies"] == 0);
    ensure!(address["data"]["activity"].as_array().is_some_and(|rows| {
        rows.iter().any(|row| {
            row["txid"] == BLOCK_ONE_TXID
                && row["blockHash"] == BLOCK_ONE_HASH
                && row["authDigest"]
                    .as_str()
                    .is_some_and(|digest| digest.len() == 64)
        })
    }));

    let network_history = get_json(&client, base_url, "/api/v1/network/history?limit=2048").await?;
    ensure!(network_history["data"]["asOfHeight"] == 48);
    ensure!(
        network_history["data"]["points"]
            .as_array()
            .is_some_and(|points| points.len() == 49)
    );

    let pools = get_json(&client, base_url, "/api/v1/value-pools/history?limit=2048").await?;
    ensure!(pools["data"]["unexpectedPoolSamples"] == 0);
    let latest_pools = pools["data"]["points"]
        .as_array()
        .and_then(|points| points.last())
        .context("value-pool history is empty")?;
    ensure!(latest_pools["transparent"]["chainValue"]["decimal"] == "300.00000000");
    ensure!(latest_pools["transparent"]["monitored"] == true);
    ensure!(latest_pools["ironwood"]["monitored"] == false);

    let merged = get_json(&client, base_url, "/api/v1/merge-mining/stats").await?;
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
        ensure!(merged["data"][field] == 48, "unexpected {field}");
    }
    ensure!(merged["data"]["anomalyBlocks"] == 0);
    ensure!(
        merged["data"]["observationSourceCount"]
            .as_i64()
            .is_some_and(|count| count >= 2)
    );

    let rich_list = get_json(&client, base_url, "/api/v1/addresses/rich-list").await?;
    ensure!(rich_list["data"]["fundedAddressCount"] == 1);
    ensure!(rich_list["data"]["addressedBalance"]["decimal"] == "300.00000000");
    ensure!(rich_list["data"]["addresses"][0]["address"] == PAYOUT_ADDRESS);
    ensure!(rich_list["data"]["addresses"][0]["transparentPoolSharePercent"] == "100.00000000");

    let address_stats = get_json(&client, base_url, "/api/v1/addresses/stats?limit=2048").await?;
    ensure!(address_stats["data"]["fundedAddressCount"] == 1);
    ensure!(address_stats["data"]["seenAddressCount"] == 1);
    ensure!(address_stats["data"]["addressedBalance"]["decimal"] == "300.00000000");

    let search = get_json(&client, base_url, "/api/v1/search?q=1").await?;
    ensure!(search["data"]["kind"] == "block");
    ensure!(search["data"]["route"] == "/block/1");

    let openapi = get_json(&client, base_url, "/api/openapi.json").await?;
    ensure!(openapi["openapi"] == "3.1.0");
    ensure!(
        openapi["paths"]
            .as_object()
            .is_some_and(|paths| paths.len() == 19)
    );
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
