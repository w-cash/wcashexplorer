# Deployment Guide

This guide describes the public Wcash Testnet explorer profile used at
[testnet.wcashexplorer.com](https://testnet.wcashexplorer.com). It is not
approved for Wcash mainnet use.

## Production-shaped topology

The primary Testnet profile keeps the explorer small and verifies the complete
AuxPoW witness already committed to each Wcash block:

```text
Internet
   │
   ▼
TLS / rate-limiting reverse proxy
   ├── /*       -> web interface
   └── /api/*   -> WcashExplorer Rust API
                         │
                         ├── PostgreSQL (private)
                         └── Wcash Testnet RPC (private)
```

This profile validates the encoded child commitment, proof branches, parent
header work, required Wcash target, and exact witness acceptance by the Wcash
node. It reports Zcash canonical-chain observation as `not_configured`; it does
not claim that the embedded parent block was observed on Zcash Testnet.

An optional strict parent-observer profile adds two private Zcash Testnet RPC
nodes. Its two parent RPC URLs must be distinct. For stronger operational
evidence, run them under separate operators, hosts, and network paths. The
application can detect agreement between configured observations, but cannot
prove that the sources are independent.

## Prerequisites

- a synchronized Wcash Testnet node with authenticated RPC;
- PostgreSQL 14 or newer with durable storage and backups;
- a host or container runtime for the Rust service;
- Node.js 22.13 or newer to build the web interface; and
- a TLS reverse proxy or ingress with request and connection limits.

The optional strict profile additionally requires two synchronized Zcash
Testnet nodes with authenticated RPC.

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

For the documented single-host Testnet topology, build Vinext's self-contained
Node artifact instead:

```sh
npm run build:standalone
test -f dist/standalone/server.js
test -f dist/standalone/standalone-runtime.json
```

`build:standalone` restores the lockfile-pinned React peer that Vinext leaves
external, then boots the artifact from an isolated temporary directory and
smoke-tests every explorer route. A successful build therefore cannot resolve
packages from the source tree by accident.

The default build remains the Cloudflare Worker artifact. Do not mix the two
outputs in one release directory.

Keep the web and API revisions together in release metadata so an operator can
identify exactly which pair is running.

## Configuration

For the primary single-host Testnet profile, start from
`deploy/testnet-wcash-only.env.example` and store the resulting environment
outside source control. The development and strict parent-observer profiles use
`.env.example`, `.env.docker.example`, or `deploy/testnet.env.example`.

Required connections:

- `DATABASE_URL`
- `WCASH_RPC_URL` plus cookie or basic-auth fields

For the optional strict profile, also configure:

- `ZCASH_RPC_URL` plus cookie or basic-auth fields; and
- `ZCASH_RPC_SECONDARY_URL` plus cookie or basic-auth fields.

Important identity and policy fields:

- `WCASH_NETWORK`, `WCASH_GENESIS_HASH`, and display/monetary parameters;
- `ZCASH_PARENT_CHAIN` and `ZCASH_PARENT_GENESIS_HASH`, which identify the
  parent chain even when no observer is configured;
- `REQUIRE_PARENT_QUORUM=false` for the primary Wcash-only profile, or `true`
  for the strict two-parent profile;
- `MAX_REORG_DEPTH` and `EVIDENCE_REFRESH_DEPTH`; and
- `RUN_MIGRATIONS` and `RUN_INDEXER`.

The checked-in testnet defaults encode eight decimals, 75-second target spacing,
a 6.25 TWC initial subsidy, first halving at height 1,680,001, and a 21 million
maximum issuance schedule. They are validated at startup. Do not copy these
values into a mainnet deployment without a separate release and review.

The concrete single-host Testnet procedure and hardened systemd unit are in
[`live-testnet-runbook.md`](live-testnet-runbook.md).

### Checked-in single-host profile

The primary profile is assembled from these reviewed templates:

- `deploy/testnet-wcash-only.env.example`: Wcash-only explorer environment;
- `deploy/wcash-testnet-explorer-node.toml.example`: private Wcash observer node;
- `deploy/systemd/wcashexplorer-start-wcash-only`: credential-loading launcher;
- `deploy/systemd/wcashexplorer-testnet-wcash-only.service`: indexer/API unit;
- `deploy/systemd/wcash-node-reconnect-delay.conf`: reconnect delay for the
  single-seed bootstrap policy;
- `deploy/systemd/wcashexplorer-web.service`: standalone web unit;
- `deploy/nginx/wcashexplorer.conf`: same-origin web/API reverse proxy for
  `testnet.wcashexplorer.com`;
- `deploy/nginx/wcash-wallet-rpc.conf`: TLS and rate-limited compact-wallet
  gRPC ingress for `wallet-testnet.wcashexplorer.com`, with node JSON-RPC kept
  private;
- `landing/` and `deploy/nginx/wcashexplorer-landing.conf`: dependency-free
  apex landing page for `wcashexplorer.com` and `www.wcashexplorer.com`, kept
  separate from the Testnet application;
- `deploy/nginx/cloudflare-real-ip.conf`: Cloudflare client-IP restoration for
  rate limiting; and
- `deploy/ssh/00-wcashexplorer-hardening.conf`: key-only SSH baseline.

The documentation-only Wcash seed in the node template must be replaced with an
operator-approved Testnet seed. Never commit the seed host if infrastructure
policy treats it as private. The older `deploy/testnet.env.example`,
`deploy/systemd/wcashexplorer-start`, and
`deploy/systemd/wcashexplorer-testnet.service` files implement the optional
strict two-parent profile.

### Apex landing page

The apex hostname is intentionally not an alias of the explorer application.
It serves the static files in `landing/` and links to the explicitly named
Testnet origin. This keeps Testnet URLs unambiguous and prevents accidental
publication of explorer routes under an unlabeled hostname.

Install the page and the existing Wcash mark with root ownership and read-only
permissions:

```sh
install -d -o root -g root -m 0755 /var/www/wcashexplorer-landing
install -o root -g root -m 0644 landing/index.html landing/styles.css \
  web/public/wcash-mark.svg web/public/og.png \
  /var/www/wcashexplorer-landing/
```

Create `/var/lib/letsencrypt/.well-known/acme-challenge`, install
`deploy/nginx/wcashexplorer-landing-bootstrap.conf` as the enabled landing
vhost, validate Nginx, and issue an independent apex certificate:

```sh
certbot certonly --webroot -w /var/lib/letsencrypt \
  --cert-name wcashexplorer.com \
  -d wcashexplorer.com -d www.wcashexplorer.com \
  --non-interactive --agree-tos
```

Replace the bootstrap vhost with
`deploy/nginx/wcashexplorer-landing.conf`, validate with `nginx -t`, and reload.
The production vhost retains the ACME webroot on HTTP and HTTPS for unattended
renewal. It uses a separate certificate lineage and never replaces the Testnet
virtual host or its certificate.

### RPC authentication

Prefer node-generated cookie files mounted read-only into the service. Set only
the corresponding `*_COOKIE_FILE` variable. If basic authentication is needed,
set both `*_USERNAME` and `*_PASSWORD` through the runtime secret store.

Never embed credentials in an RPC URL. The application rejects URL userinfo.
Never bake cookies or passwords into an image, Compose file, shell history, or
repository.

### Compact-wallet ingress

The optional public wallet endpoint forwards only the
`cash.z.wallet.sdk.rpc.CompactTxStreamer` gRPC service from
`wallet-testnet.wcashexplorer.com` to the node's loopback compact-block port.
It does not expose authenticated JSON-RPC, PostgreSQL, or the explorer API.

Create a DNS-only A record for the wallet hostname, issue its certificate with
the existing ACME webroot, then install
`deploy/nginx/wcash-wallet-rpc.conf`. Validate the live endpoint with
`GetLightdInfo` and the exact Wcash Testnet genesis before compiling it into a
wallet release.

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

1. the synchronized Wcash Testnet RPC node;
2. PostgreSQL;
3. database migration job, if separated;
4. one Rust indexer/API process;
5. optional read-only API replicas; and
6. web interface and public ingress.

Start both Zcash Testnet RPC nodes before the indexer only when deploying the
optional strict parent-observer profile.

The service always validates Wcash chain identity before indexing. It also
validates every configured Zcash observer's chain identity. Do not route public
traffic merely because `/health/live` succeeds.

Use:

- `/health/live` for process liveness; and
- `/health/ready` for traffic readiness.

Readiness requires a fresh indexer heartbeat, a valid canonical tip, and no
more than one block of lag. In the primary Wcash-only profile,
`REQUIRE_PARENT_QUORUM=false`; readiness does not depend on an external parent
observer and parent observation remains `not_configured`. With
`REQUIRE_PARENT_QUORUM=true`, every non-genesis canonical tip must also have at
least two fresh parent observations; all must be canonical, agree, and match the
embedded parent header. The indexer prioritizes the tip for refresh after 30
seconds, and the readiness budget is 60 seconds so one missed refresh does not
flap traffic. Parent uncertainty makes the strict probe fail but does not stop
local Wcash indexing. Remove an instance from service immediately when
readiness fails; historical API reads remain available for diagnosis.

## Verification before ASIC or public test traffic

Before announcing the explorer:

1. confirm the Wcash RPC endpoint reports the expected test network and genesis
   block;
2. index from genesis into an empty database;
3. compare tip height and hash with the Wcash node;
4. inspect a known AuxPoW block and confirm:
   - local state is `auxpow_verified`;
   - exact witness state is `best_chain`;
   - the parent hash and parent coinbase ID match the proof;
   - parent lookup is `not_configured` and no canonical-parent claim is shown;
5. exercise block, transaction, transparent-address, raw-block, AuxPoW, search,
   and pagination routes;
6. test a Wcash reorganization in an isolated environment and verify readers
   never observe a partially replaced branch;
7. restart PostgreSQL and verify the writer exits on lease loss and recovers only
   through the supervisor; and
8. run the Rust and web validation commands from the README.

For the optional strict parent-observer profile, additionally confirm both
Zcash RPC endpoints report the expected parent network and genesis, every
configured observation has an exact embedded-header match and fresh timestamp,
and stopping one parent node changes the result to uncertainty rather than
retaining a misleading agreement state.

A successful local fixture test does not replace these live testnet checks.

## Monitoring

Collect at least:

- liveness, readiness, and readiness error rate;
- indexed height, node height, lag, and heartbeat age;
- canonical tip hash mismatches at equal height;
- indexer restarts and writer-lock failures;
- reorg depth and duration;
- RPC latency, timeouts, and bounded-response failures per source;
- parent observation state, header-match result, and age when parent observers
  are configured; and
- PostgreSQL capacity, replication, backup, and statement-timeout events.

Do not put RPC credentials, cookies, full authorization headers, private
hostnames, or database URLs into logs or metrics labels.

## Recovery and rollback

- **Application rollback:** deploy the prior backend and web revisions together.
  Do not roll back across an incompatible database migration without its
  documented recovery procedure.
- **Indexer failure:** keep public traffic off the instance, resolve the RPC or
  database fault, then restart one writer. Do not bypass the advisory lock.
- **Parent disagreement (optional observer profile):** preserve the evidence,
  remove any unqualified parent-canonical presentation, investigate the sources,
  and wait for convergence. Local Wcash AuxPoW validity remains a separate
  claim.
- **Database loss:** restore a tested backup, validate network identity, and
  reindex forward while readiness remains disabled.
- **Unexpected deep reorg:** stop indexing and investigate. Raising
  `MAX_REORG_DEPTH` is an explicit operator decision, not an automatic remedy.

## Current release boundary

This repository currently targets the public Wcash Testnet explorer at
[testnet.wcashexplorer.com](https://testnet.wcashexplorer.com). Production
retention policy, optional independent parent-source operations, load testing,
disaster-recovery rehearsal, and a separate mainnet release review remain
deployment work. Do not describe the Testnet service or source tree as a
mainnet-ready explorer.
