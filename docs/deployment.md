# Deployment Guide

This guide describes a private Wcash Testnet deployment for evaluation. The
repository has not yet been deployed publicly and is not approved for Wcash
mainnet use.

## Production-shaped topology

Use separate failure and trust domains where practical:

```text
Internet
   │
   ▼
TLS / rate-limiting reverse proxy
   ├── /*       -> web interface
   └── /api/*   -> WcashExplorer Rust API
                         │
                         ├── PostgreSQL (private)
                         ├── Wcash Testnet RPC (private)
                         ├── Zcash Testnet RPC A (private)
                         └── Zcash Testnet RPC B (private)
```

The two parent RPC URLs must be distinct. For stronger operational evidence,
run them under separate operators, hosts, and network paths. The application can
detect agreement between configured observations, but cannot prove that the
sources are independent.

## Prerequisites

- a synchronized Wcash Testnet node with authenticated RPC;
- two synchronized Zcash Testnet nodes with authenticated RPC;
- PostgreSQL 17 with durable storage and backups;
- a host or container runtime for the Rust service;
- Node.js 22.13 or newer to build the web interface; and
- a TLS reverse proxy or ingress with request and connection limits.

Do not expose node RPC, PostgreSQL, cookie files, or management ports to the
public internet.

## Build artifacts

Build the backend from the locked dependency graph:

```sh
cargo build --locked --release
```

Or build the included non-root container image:

```sh
docker build --pull -t wcashexplorer:local .
```

Build the web interface separately:

```sh
cd web
npm ci
npm run lint
npm run build
```

Keep the web and API revisions together in release metadata so an operator can
identify exactly which pair is running.

## Configuration

Start from `.env.example` for a native process or `.env.docker.example` for the
development Compose topology. Store the resulting environment outside source
control.

Required connections:

- `DATABASE_URL`
- `WCASH_RPC_URL` plus cookie or basic-auth fields
- `ZCASH_RPC_URL` plus cookie or basic-auth fields
- `ZCASH_RPC_SECONDARY_URL` plus cookie or basic-auth fields

Important identity and policy fields:

- `WCASH_NETWORK`, `WCASH_GENESIS_HASH`, and display/monetary parameters;
- `ZCASH_PARENT_CHAIN` and `ZCASH_PARENT_GENESIS_HASH`;
- `REQUIRE_PARENT_QUORUM=true` for the intended testnet configuration;
- `MAX_REORG_DEPTH` and `EVIDENCE_REFRESH_DEPTH`; and
- `RUN_MIGRATIONS` and `RUN_INDEXER`.

The checked-in testnet defaults encode eight decimals, 75-second target spacing,
a 6.25 tWEC initial subsidy, first halving at height 1,680,001, and a 21 million
maximum issuance schedule. They are validated at startup. Do not copy these
values into a mainnet deployment without a separate release and review.

### RPC authentication

Prefer node-generated cookie files mounted read-only into the service. Set only
the corresponding `*_COOKIE_FILE` variable. If basic authentication is needed,
set both `*_USERNAME` and `*_PASSWORD` through the runtime secret store.

Never embed credentials in an RPC URL. The application rejects URL userinfo.
Never bake cookies or passwords into an image, Compose file, shell history, or
repository.

### Bind and ingress

Bind the Rust API to a private interface. The development Compose file publishes
it on loopback only. Ingress should:

- terminate TLS;
- forward `/api/*` without exposing node RPC;
- apply per-IP connection and request-rate limits;
- preserve or generate a request ID;
- impose header and request timeouts; and
- return no internal hostnames or upstream error bodies.

Serve the web interface and API on one origin when possible. If they must use
different origins, add an explicitly reviewed CORS policy; the Rust service does
not enable permissive CORS by default.

Do not set `NEXT_PUBLIC_EXPLORER_PREVIEW=true` in a public build. Preview mode is
only for a clearly labeled local design snapshot. For a separate API origin,
set `NEXT_PUBLIC_EXPLORER_API_BASE` to a reviewed public HTTPS origin at build
time. Same-origin `/api` routing is preferred.

## Database lifecycle

The service can run embedded, checksummed migrations at startup when
`RUN_MIGRATIONS=true`. For a controlled rollout:

1. back up PostgreSQL and record the application revision;
2. run one migration-capable instance with the indexer disabled;
3. inspect migration completion;
4. start exactly one indexer instance; and
5. scale read-only API instances only with `RUN_INDEXER=false`.

The writer uses a session-level PostgreSQL advisory lock. Starting a second
indexer against the same database should fail; losing the lock causes the writer
process to stop. A supervisor should restart it only after PostgreSQL is healthy.

Back up the database even though much canonical data can be reindexed. Historical
orphan observations and exact witness evidence may be harder to reacquire after
upstream pruning or configuration changes.

## Start and readiness gates

Start dependencies in this order:

1. synchronized Wcash and both Zcash Testnet RPC nodes;
2. PostgreSQL;
3. database migration job, if separated;
4. one Rust indexer/API process;
5. optional read-only API replicas; and
6. web interface and public ingress.

The service validates Wcash and Zcash chain identity before indexing. Do not
route public traffic merely because `/health/live` succeeds.

Use:

- `/health/live` for process liveness; and
- `/health/ready` for traffic readiness.

Readiness requires a fresh indexer heartbeat, a valid canonical tip, and no
more than one block of lag. Remove an instance from service immediately when
readiness fails. Alert separately on parent `disagreement`, `unavailable`, or
stale observation times even when historical API reads still work.

## Verification before ASIC or public test traffic

Before announcing the explorer:

1. confirm all three RPC endpoints report the expected test networks and genesis
   blocks;
2. index from genesis into an empty database;
3. compare tip height and hash with the Wcash node;
4. inspect a known AuxPoW block and confirm:
   - local state is `auxpow_verified`;
   - exact witness state is `best_chain`;
   - the parent hash and parent coinbase ID match the proof;
   - every configured parent observation reports an exact header match; and
   - parent observation timestamps are fresh;
5. exercise block, transaction, transparent-address, raw-block, AuxPoW, search,
   and pagination routes;
6. test a Wcash reorganization in an isolated environment and verify readers
   never observe a partially replaced branch;
7. stop one parent node and verify the explorer reports uncertainty rather than
   retaining a misleading agreement state;
8. restart PostgreSQL and verify the writer exits on lease loss and recovers only
   through the supervisor; and
9. run the Rust and web validation commands from the README.

A successful local fixture test does not replace these live testnet checks.

## Monitoring

Collect at least:

- liveness, readiness, and readiness error rate;
- indexed height, node height, lag, and heartbeat age;
- canonical tip hash mismatches at equal height;
- indexer restarts and writer-lock failures;
- reorg depth and duration;
- RPC latency, timeouts, and bounded-response failures per source;
- parent observation state, header-match result, and age; and
- PostgreSQL capacity, replication, backup, and statement-timeout events.

Do not put RPC credentials, cookies, full authorization headers, private
hostnames, or database URLs into logs or metrics labels.

## Recovery and rollback

- **Application rollback:** deploy the prior backend and web revisions together.
  Do not roll back across an incompatible database migration without its
  documented recovery procedure.
- **Indexer failure:** keep public traffic off the instance, resolve the RPC or
  database fault, then restart one writer. Do not bypass the advisory lock.
- **Parent disagreement:** preserve the evidence, remove any unqualified
  “verified” presentation, investigate the sources, and wait for convergence.
- **Database loss:** restore a tested backup, validate network identity, and
  reindex forward while readiness remains disabled.
- **Unexpected deep reorg:** stop indexing and investigate. Raising
  `MAX_REORG_DEPTH` is an explicit operator decision, not an automatic remedy.

## Current release boundary

This repository currently targets a Wcash Testnet explorer. Domain configuration,
public TLS, production retention policy, independent parent-source operations,
load testing, disaster-recovery rehearsal, and a mainnet release review remain
deployment work. Do not describe the source tree alone as a live explorer or a
mainnet-ready service.
