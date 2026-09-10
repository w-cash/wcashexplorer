# Contributing to WcashExplorer

WcashExplorer welcomes focused, reviewable contributions. The current target is
a trustworthy Wcash Testnet explorer; changes must not imply public deployment
or mainnet readiness.

## Before starting

Open an issue for changes that affect the database schema, public API, AuxPoW
evidence semantics, privacy language, or deployment model. Small fixes can go
directly to a pull request.

Keep a branch limited to one coherent change. Use professional, imperative
commit subjects such as `Harden parent-chain evidence refresh`.

## Development setup

Follow the local setup in [README.md](README.md). Never commit `.env` files,
cookies, RPC credentials, private keys, wallet material, database dumps, or
infrastructure addresses.

## Required checks

Run the checks relevant to your change before opening a pull request:

```sh
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings

cd web
npm ci
npm run lint
npm run build
```

Consensus-adjacent changes need positive and negative tests. Prefer fixed,
reviewable fixtures. Do not make a test depend on a public RPC service or a live
chain tip.

## Design rules

- PostgreSQL is authoritative. A cache may accelerate reads, but cannot select
  a canonical chain or become necessary for correctness.
- Keep immutable chain facts separate from canonical selectors and mutable
  observations.
- Publish a reorganization atomically. Readers must not observe a half-replaced
  branch.
- Preserve both the Wcash block hash and witness hash. Do not treat the block ID
  as an identifier for the exact AuxPoW bytes.
- Preserve transaction identity as `(txid, authDigest)` and retain block context
  where an API route could otherwise be ambiguous.
- Bound RPC responses, request work, pagination, and database statements.
- Never proxy node RPC through the public API.
- Keep monetary amounts exact in atomic units and serialize them as strings.

## Evidence language

Do not collapse these claims into one “verified” flag:

- local AuxPoW proof validation;
- exact-witness acceptance by the configured Wcash node;
- canonical, orphaned, missing, or unavailable observation by each configured
  Zcash parent node; and
- agreement between configured parent observations.

Source agreement is not proof that sources are independently operated. User
interface and API changes must preserve that distinction.

## Privacy language

Only transparent addresses are searchable. Never infer or label a shielded
sender, recipient, address, per-note value, balance, or memo. Public shielded
artifacts such as commitments, nullifiers, ciphertexts, value-pool totals, and
action counts may be displayed only with clear language explaining what they do
and do not reveal.

See [docs/privacy-model.md](docs/privacy-model.md) before changing transaction or
address views.

## Database migrations

Add a new migration for every schema change; do not rewrite a migration that may
have been applied elsewhere. Migrations must be forward-only, transactional
where PostgreSQL permits it, and safe against existing indexed data. Include a
rollback or recovery procedure in the pull-request description even when the
migration itself is irreversible.

## Pull requests

Describe:

- the user or operator problem being solved;
- any consensus, evidence, privacy, or schema implications;
- tests performed and their results;
- operational rollout and rollback considerations; and
- remaining limitations.

By contributing, you agree that your contribution may be licensed under either
Apache-2.0 or MIT, at the recipient's option.
