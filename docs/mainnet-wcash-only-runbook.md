# Wcash Mainnet explorer: same host, separate chain

The Mainnet origin is `https://wcashexplorer.com/`. Keep
`https://testnet.wcashexplorer.com/` running independently. This profile indexes
**only Wcash Mainnet**: the Rust service validates the complete AuxPoW witness
locally and checks its exact acceptance by the Wcash node. It does not run,
query, or download Zcash Mainnet. Parent canonical-chain observation is
`not_configured`; never describe it as independently confirmed on Zcash.

## Hard identity gate

- Build the Mainnet node from Wolf revision `45393319d24c3c3181f98d503c97a315661b59af`
  or a reviewed descendant of genesis commit `8187216f4fe4d337c2f441c84193081a2143436d`
  with the **same genesis hash**.
- Expected Wcash Mainnet genesis:
  `5bae12c8662a577b04ce1591af1a137c128f0cb51018a5f1622d861d1bb6fc48`.
- Expected peer: `mainnet.zecwec.com:48233`. This is a Wcash peer, not Zcash.
- Require `getblockchaininfo.chain = main`, `getblockhash(0)` equal to the
  above hash, and the indexer's `/api/v1/status` report `network=mainnet`,
  `symbol=WEC`. Genesis equality is more important than the hostname resolving.
- Do not publish Mainnet web routing before the Mainnet API is ready. A synced
  genesis-only node can be a valid starting state; label its height and lack of
  activity honestly.

## Isolated services

| Component | Mainnet | Existing Testnet |
| --- | --- | --- |
| Wcash P2P | `127.0.0.1:48233` | `127.0.0.1:38233` |
| Wcash RPC | `127.0.0.1:48232` | `127.0.0.1:38232` |
| Node state | `/srv/wcash-mainnet` | `/srv/wcash-testnet` |
| API | `127.0.0.1:8081` | `127.0.0.1:8080` |
| Web | `127.0.0.1:3002` | `127.0.0.1:3001` |
| PostgreSQL DB | `wcashexplorer_mainnet` | `wcashexplorer` |
| Public host | `wcashexplorer.com` | `testnet.wcashexplorer.com` |

Use separate Unix service users and a separate PostgreSQL peer role/database.
The indexer's advisory writer lock is database-scoped, so do **not** share the
Testnet database between two writer processes. Preserve its backups and state.
Neither node RPC, database socket, nor API/web loopback ports are public.

The reviewed templates are:

- `deploy/wcash-mainnet-explorer-node.toml.example`;
- `deploy/mainnet-wcash-only.env.example`;
- `deploy/systemd/wcash-mainnet-node.service`;
- `deploy/systemd/wcashexplorer-mainnet.service` and
  `deploy/systemd/wcashexplorer-start-mainnet`;
- `deploy/systemd/wcashexplorer-mainnet-web.service`;
- `deploy/nginx/wcashexplorer-mainnet.conf`.

The deployed Mainnet environment has `REQUIRE_PARENT_QUORUM=false`, no
`ZCASH_RPC_URL`, and empty external parent-explorer URL templates. Keep those
settings if the host must remain Wcash-only. Never use the older strict
Testnet parent-observer service for Mainnet.

Build the Rust binary from the committed lockfile with `cargo build --locked
--release`. Build a **separate** web artifact with
`NEXT_PUBLIC_WCASH_NETWORK=mainnet npm run build:standalone`; the existing
Testnet artifact continues to use its Testnet default. The browser rejects an
API response whose `meta.network` differs from the web artifact's network.
The Mainnet artifact must not use `NEXT_PUBLIC_EXPLORER_PREVIEW=true`.

## Activation order

1. Back up PostgreSQL, the existing Testnet services, and the apex Nginx
   landing-vhost configuration. Record installed binary digests and revisions.
2. Create the Mainnet service users and dedicated PostgreSQL role/database;
   install the Wolf binary and separate node/config/state paths.
3. Start the private Mainnet node and confirm `main` plus the exact frozen
   genesis. Wait for its height/hash to converge with the trusted Wcash peer.
4. Install the Mainnet explorer binary/environment/credential launcher and
   start its API/indexer. Check loopback `/health/live`, `/health/ready`,
   `/api/v1/status`, `/api/v1/blocks/0`, and `/api/v1/merge-mining/stats`.
   Check the genesis hash and `parentLookupState=not_configured` for an
   AuxPoW block. Do not invent a parent observation.
5. Install the Mainnet web artifact and start it on `127.0.0.1:3002`. Confirm
   Mainnet badge, WEC values, correct API network, and the Testnet switch link.
6. Replace **only** the apex landing Nginx vhost with
   `deploy/nginx/wcashexplorer-mainnet.conf`; do not disable or modify the
   Testnet vhost. `nginx -t` before reload; keep the old apex file for rollback.
7. Verify both HTTPS origins independently, including block/address/search
   routes and TLS/ACME renewal. Mainnet and Testnet must never display one
   another's data if one upstream is down or misrouted.

If the node's chain/genesis or the web API network differs, stop before the
apex cutover. If the apex cutover fails, restore only the old apex vhost and
reload Nginx; leave Testnet running throughout.
