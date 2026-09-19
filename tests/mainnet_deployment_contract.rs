//! Catches cross-chain wiring mistakes in the separately deployed Mainnet profile.

const ENV: &str = include_str!("../deploy/mainnet-wcash-only.env.example");
const NODE: &str = include_str!("../deploy/wcash-mainnet-explorer-node.toml.example");
const NODE_UNIT: &str = include_str!("../deploy/systemd/wcash-mainnet-node.service");
const EXPLORER_UNIT: &str = include_str!("../deploy/systemd/wcashexplorer-mainnet.service");
const WEB_UNIT: &str = include_str!("../deploy/systemd/wcashexplorer-mainnet-web.service");
const NGINX: &str = include_str!("../deploy/nginx/wcashexplorer-mainnet.conf");

#[test]
fn mainnet_profile_is_wcash_only_and_genesis_pinned() {
    assert!(ENV.contains("WCASH_NETWORK=mainnet\n"));
    assert!(ENV.contains("WCASH_SYMBOL=WEC\n"));
    assert!(ENV.contains(
        "WCASH_GENESIS_HASH=5bae12c8662a577b04ce1591af1a137c128f0cb51018a5f1622d861d1bb6fc48\n"
    ));
    assert!(ENV.contains("REQUIRE_PARENT_QUORUM=false\n"));
    assert!(ENV.contains("ZCASH_PARENT_CHAIN=main\n"));
    assert!(ENV.contains("ZCASH_PARENT_GENESIS_HASH=00040fe8ec8471911baa1db1266ea15dd06b4a8a5c453883c000b031973dce08\n"));
    assert!(!ENV.lines().any(|line| line.starts_with("ZCASH_RPC_URL=")));
    assert!(
        !ENV.lines()
            .any(|line| line.starts_with("ZCASH_RPC_SECONDARY_URL="))
    );
    assert!(NODE.contains("network = \"WcashMainnet\""));
    assert!(NODE.contains("initial_mainnet_peers = [\"mainnet.zecwec.com:48233\"]"));
}

#[test]
fn mainnet_services_do_not_reuse_testnet_state_or_listeners() {
    assert!(ENV.contains("WCASH_EXPLORER_BIND=127.0.0.1:8081"));
    assert!(ENV.contains("WCASH_RPC_URL=http://127.0.0.1:48232"));
    assert!(ENV.contains("postgresql:///wcashexplorer_mainnet?host=/var/run/postgresql"));
    assert!(NODE.contains("cache_dir = \"/srv/wcash-mainnet/wcash-cache\""));
    assert!(NODE.contains("cookie_dir = \"/run/wcash-mainnet-rpc\""));
    assert!(NODE_UNIT.contains("/srv/wcash-mainnet/config/wcash-mainnet.toml start"));
    assert!(EXPLORER_UNIT.contains("LoadCredential=wcash_rpc:/run/wcash-mainnet-rpc/.cookie"));
    assert!(WEB_UNIT.contains("Environment=PORT=3002"));
    assert!(NGINX.contains("server 127.0.0.1:3002;"));
    assert!(NGINX.contains("server 127.0.0.1:8081;"));
    assert!(!NGINX.contains("server_name testnet.wcashexplorer.com"));
}
