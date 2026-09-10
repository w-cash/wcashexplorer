# Live Wcash Testnet explorer runbook

This runbook deploys one private indexer/API process beside the existing Wcash
and Zcash Testnet nodes. It does not expose node RPC or PostgreSQL publicly.

## Host preparation

Install PostgreSQL, POSIX ACL support, and the Rust build prerequisites. Create a
locked service account for the explorer process:

```sh
useradd --system --home /nonexistent --shell /usr/sbin/nologin wcashexplorer
```

Create a PostgreSQL login and database with the same name as the operating-system
account. Local peer authentication then works without a stored database password:

```sh
sudo -u postgres createuser wcashexplorer
sudo -u postgres createdb --owner=wcashexplorer wcashexplorer
```

## Install an exact revision

Build with the locked dependency graph. Install the executable atomically so a
failed copy cannot replace a running release:

```sh
cargo build --locked --release
install -D -o root -g root -m 0755 target/release/wcashexplorer \
  /opt/wcashexplorer/bin/wcashexplorer.new
mv /opt/wcashexplorer/bin/wcashexplorer.new \
  /opt/wcashexplorer/bin/wcashexplorer
```

Record the deployed Git commit in the operator log. Never build from an
uncommitted working tree.

## Configure and start

Copy `deploy/testnet.env.example` to `/etc/wcashexplorer/testnet.env`, owned by
`root:wcashexplorer` with mode `0640`. Copy the systemd unit to
`/etc/systemd/system/wcashexplorer-testnet.service`, and install
`deploy/systemd/wcashexplorer-start` at
`/opt/wcashexplorer/libexec/wcashexplorer-start` with mode `0755`.

The unit waits for PostgreSQL and all three node processes. systemd reads the
owner-only node cookies and exposes private copies in the explorer's per-service
credential directory. Restarting a node propagates a restart to the explorer,
which reloads the new credential. The source cookie modes and ownership are never
weakened. The unit also restricts IP traffic to localhost.

```sh
systemd-analyze verify /etc/systemd/system/wcashexplorer-testnet.service
systemctl daemon-reload
systemctl enable --now wcashexplorer-testnet.service
```

## Acceptance gate

Keep the API on loopback until all checks pass:

```sh
curl --fail --silent http://127.0.0.1:8080/health/live
curl --fail --silent http://127.0.0.1:8080/health/ready
curl --fail --silent http://127.0.0.1:8080/api/v1/status
curl --fail --silent 'http://127.0.0.1:8080/api/v1/blocks?limit=5'
```

Then verify the indexed tip hash against the Wcash node, open a known block and
its AuxPoW evidence, open its coinbase transaction, and open the Wcash transparent
mining address. A Zcash `tm...` parent payout address is not a Wcash address and
must not be presented as one.

For local UI validation, forward the loopback API over SSH and start the web
development server without preview mode:

```sh
ssh -N -L 18080:127.0.0.1:8080 explorer-host
cd web
EXPLORER_DEV_API_PROXY=http://127.0.0.1:18080 npm run dev -- --port 3001
```

Do not publish the service until readiness, pagination, missing-record responses,
and privacy labels have been checked against live data. Public ingress should use
TLS, explicit rate limits, and same-origin `/api/*` proxying.
