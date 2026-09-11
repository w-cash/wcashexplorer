# WcashExplorer

WcashExplorer is a read-only block explorer for **Wcash Testnet**. It combines a
Rust indexer and HTTP API with a responsive web interface, independently
validates each on-chain Wcash AuxPoW witness, and can optionally record
observations of the corresponding Zcash Testnet parent block.

The public Testnet explorer is
[testnet.wcashexplorer.com](https://testnet.wcashexplorer.com). This repository
is under active development, and the Testnet deployment is not a statement that
Wcash mainnet is ready. Testnet coins (`TWC`) have no monetary value.

## What it provides

- canonical Wcash blocks and transaction instances;
- exact monetary values as decimal strings, with eight decimal places;
- transparent address activity and UTXO-derived balances;
- aggregate public value-pool data and shielded action counts;
- reorganization-aware indexing and recorded reorg events;
- raw block retrieval and exact AuxPoW witness identity;
- local AuxPoW verification and exact Wcash witness acceptance;
- optional observations from configured Zcash Testnet RPC nodes;
- liveness and strict readiness endpoints for operators.

The explorer does **not** reveal shielded senders, recipients, note values,
balances, or memos. It does not attempt to guess them. See
[the privacy model](docs/privacy-model.md).

## Evidence, not a single badge

Merge-mining evidence is intentionally split into distinct claims:

1. **Local AuxPoW validation** verifies the serialized proof against the Wcash
   block candidate and required target using a pinned Wcash verifier revision.
2. **Exact Wcash witness state** asks the Wcash node about the exact block hash
   and solution bytes; a non-genesis block is indexed only when that witness is
   reported as `best_chain`.
3. **Optional parent-chain observation** checks the embedded parent header and
   exact parent hash against each configured Zcash Testnet node. When no parent
   observer is configured, the API reports this layer as `not_configured`; that
   does not invalidate the independently verified on-chain Wcash proof.
   Agreement means only that the configured observations agree. It does not
   imply that the node operators are independent or that the explorer is a
   consensus oracle.

These states can change independently as either chain reorganizes or an RPC
source becomes unavailable. Recent evidence is refreshed without rewriting the
canonical Wcash selection.

## Repository layout

```text
src/            Rust indexer, AuxPoW verifier, PostgreSQL store, and Axum API
migrations/     Checksummed PostgreSQL schema migrations
tests/fixtures/ Real Wcash Testnet AuxPoW fixtures used by verifier tests
web/            Vinext/React explorer interface
docs/           Architecture, privacy, and deployment documentation
```

## Requirements

- Rust 1.88 or newer;
- PostgreSQL 17 (the development Compose file supplies it);
- Node.js 22.13 or newer for the web interface;
- one Wcash Testnet RPC endpoint; and
- optionally, two distinct Zcash Testnet RPC endpoints for strict
  parent-observation quorum.

The Wcash RPC service, and any optional parent RPC services, must be fully
synchronized to their expected networks. Keep every node RPC listener private
and authenticated.

## Local development

Start PostgreSQL:

```sh
docker compose up -d postgres
```

Create local configuration and replace the RPC paths or credentials with your
own private development endpoints:

```sh
cp .env.example .env
```

Run the Rust API and indexer:

```sh
cargo run
```

In another terminal, start the web interface with its same-origin development
proxy:

```sh
cd web
npm ci
EXPLORER_DEV_API_PROXY=http://127.0.0.1:8080 npm run dev
```

The development example enables the stricter two-parent observer profile. The
lean public-Testnet deployment profile is documented in
[`docs/live-testnet-runbook.md`](docs/live-testnet-runbook.md) and requires only
one Wcash node. Both profiles are pinned to the expected chain identities;
startup fails if a configured chain identity or the database's stored network
identity differs.

## Validation

```sh
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings

cd web
npm run lint
npm run build
```

The Rust tests include a real Wcash Testnet block fixture with its serialized
AuxPoW solution. A green unit-test run is necessary, but it is not a substitute
for an end-to-end test against synchronized testnet nodes.

## HTTP surface

The public API is read-only and rooted at `/api/v1`:

- `GET /status` and `GET /stats`
- `GET /blocks` and `GET /blocks/{height-or-hash}`
- `GET /blocks/{height-or-hash}/raw`
- `GET /blocks/{height-or-hash}/auxpow`
- `GET /transactions` and `GET /transactions/{txid}`
- `GET /addresses/{transparent-address}`
- `GET /addresses/stats` and `GET /addresses/rich-list`
- `GET /network/history` and `GET /value-pools/history`
- `GET /merge-mining/stats`
- `GET /reorgs`
- `GET /search?q=...`

Pagination uses opaque cursors. Atomic amounts are serialized as strings so
JavaScript clients cannot silently lose precision. Transaction identity retains
`txid`, an internal instance fingerprint, and the ZIP-244 authorization digest
when that transaction version defines one. Callers may provide a block hash
when resolving a transaction instance. Analytics responses are anchored to
one repeatable-read canonical snapshot, and detached reorganization data is
excluded from public search, balances, histories, and rankings.

Operational probes are available at `GET /health/live` and
`GET /health/ready`. Readiness requires a fresh heartbeat, a valid canonical
tip, and at most one block of indexing lag. With the Wcash-only profile,
`REQUIRE_PARENT_QUORUM=false` and no Zcash parent RPC is configured, so
readiness depends only on Wcash chain and indexer state; parent observation is
reported as `not_configured`. When `REQUIRE_PARENT_QUORUM=true`, every
non-genesis tip also requires fresh, agreeing canonical evidence from at least
two parent sources. The tip is prioritized for evidence refresh after 30
seconds; readiness allows one missed refresh and fails once that evidence is
older than 60 seconds. Historical indexing continues while the strict probe is
unready.

## Storage

PostgreSQL is the authoritative explorer index. Immutable block, witness, and
transaction facts are separated from the mutable canonical-chain selector so a
reorganization can be published atomically.

Redis is deliberately not part of the first production design. Adding another
stateful layer before profiling would add invalidation and reorg-consistency
risk. If measurements later show a cache is needed, it should remain disposable
and must never become an authority for chain state.

For the full design, see [architecture](docs/architecture.md). For an operator
checklist, see [deployment](docs/deployment.md).

## Security and contributions

Report vulnerabilities through the process in [SECURITY.md](SECURITY.md). Code
and documentation contributions are described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)); or
- MIT License ([LICENSE-MIT](LICENSE-MIT)).

at your option.
