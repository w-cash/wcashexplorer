# Live Wcash Testnet explorer runbook

This runbook deploys the primary Wcash-only observer profile used for
[testnet.wcashexplorer.com](https://testnet.wcashexplorer.com): one private
Wcash Testnet node, PostgreSQL, one indexer/API process, the web process, and
Nginx. It independently validates the AuxPoW witness carried by Wcash blocks
without running a Zcash node. It does not expose node RPC or PostgreSQL
publicly.

This profile deliberately reports Zcash canonical-chain observation as
`not_configured`. Operators who require independent evidence that an embedded
parent is canonical on Zcash Testnet can instead deploy the optional strict
two-parent profile documented in [`deployment.md`](deployment.md).

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

The explorer repository does not build or install the consensus node. First
install `wcash-zebrad` from the reviewed Wcash node revision
[`72038cee`](https://github.com/w-cash/wolf/tree/72038ceef70b297a1ef771177a3f45a88f53474c),
which is also the AuxPoW dependency revision pinned by this release. Install an
operator-owned base unit named `wcash-testnet-node.service`; the upstream unit
source is
[`zebrad/systemd/zebrad.service`](https://github.com/w-cash/wolf/blob/72038ceef70b297a1ef771177a3f45a88f53474c/zebrad/systemd/zebrad.service).
The base unit must exist before installing the explorer-specific drop-in below.

Install `deploy/wcash-testnet-explorer-node.toml.example` as
`/srv/wcash-testnet/config/wcash-testnet.toml`. Replace its documentation-only
seed with an operator-approved Wcash Testnet peer, and keep its P2P, RPC, and
lightwallet listeners on loopback. Configure the base unit to run the reviewed
binary with `-c /srv/wcash-testnet/config/wcash-testnet.toml` under a locked
service account. Install
`deploy/systemd/wcash-node-reconnect-delay.conf` as
`/etc/systemd/system/wcash-testnet-node.service.d/reconnect-delay.conf`. The
delay avoids repeatedly reconnecting inside the Testnet seed's per-IP cooldown.

Copy `deploy/testnet-wcash-only.env.example` to
`/etc/wcashexplorer/testnet.env`, owned by `root:wcashexplorer` with mode `0640`.
Copy `deploy/systemd/wcashexplorer-testnet-wcash-only.service` to
`/etc/systemd/system/wcashexplorer-testnet.service`, and install
`deploy/systemd/wcashexplorer-start-wcash-only` at
`/opt/wcashexplorer/libexec/wcashexplorer-start-wcash-only` with mode `0755`.

The explorer unit waits only for PostgreSQL and the Wcash node. systemd reads
the owner-only Wcash RPC cookie and exposes a private copy in the explorer's
per-service credential directory. Restarting the node propagates a restart to
the explorer, which reloads the new credential. The source cookie mode and
ownership are never weakened. The unit also restricts IP traffic to localhost.

```sh
systemd-analyze verify /etc/systemd/system/wcash-testnet-node.service \
  /etc/systemd/system/wcashexplorer-testnet.service
systemctl daemon-reload
systemctl enable --now wcash-testnet-node.service
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
must not be presented as one. The AuxPoW view must show local verification and
exact Wcash witness state. It must show parent-chain observation as
`not_configured`, not as Zcash-canonical, when this Wcash-only profile is active.

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

## Install the public web artifact

Build a self-contained web release from the same reviewed Git revision as the
Rust API. Keep preview mode disabled and preserve same-origin API URLs:

```sh
cd web
npm ci
npm run lint
npm run build:standalone
test -f dist/standalone/server.js
test -f dist/standalone/standalone-runtime.json
```

The standalone build restores Vinext's lockfile-pinned external React peer and
boots a copy from an isolated temporary directory. Do not publish an artifact
if that automatic route smoke test fails.

Copy the **contents** of `dist/standalone/` into a new, immutable release
directory below `/opt/wcashexplorer-web/releases/`; the resulting layout must
place the entry point at `<release>/server.js` and its bundled files at
`<release>/dist/` and `<release>/node_modules/`. Do not nest another
`standalone/` directory. Do not copy a developer environment, source
`node_modules`, `.env` files, SSH material, or RPC credentials. Verify
`<release>/server.js`, then point `/opt/wcashexplorer-web/current` at that exact
release with one atomic symlink replacement.

Install an official Node.js 22 runtime under `/opt/wcashexplorer-web/node`, create
the locked `wcashexplorer-web` service account, and install
`deploy/systemd/wcashexplorer-web.service`. The web process listens only on
`127.0.0.1:3001` and has no access to node RPC credentials or PostgreSQL.

Install `deploy/nginx/wcashexplorer.conf` as
`/etc/nginx/sites-available/wcashexplorer.conf`, then symlink that complete file
from `/etc/nginx/sites-enabled/`. It must be included from Nginx's `http`
context, never from inside another `server` block, because it defines shared
request zones and upstreams. The ingress keeps the browser and read-only API on
one origin, applies separate request limits, and exposes neither the node RPC
ports nor PostgreSQL.

When Cloudflare proxies the public hostname, install
`deploy/nginx/cloudflare-real-ip.conf` as
`/etc/nginx/conf.d/cloudflare-real-ip.conf`. It restores the client address used
by access logs and rate limits from `CF-Connecting-IP`, but only for the listed
Cloudflare source networks. Compare those networks with Cloudflare's official
IP lists before each infrastructure release; never add an untrusted source
range. The origin must still use HTTPS and must not rely on this header for
application authentication. Validate before reload:

```sh
systemd-analyze verify /etc/systemd/system/wcashexplorer-web.service
nginx -t
systemctl daemon-reload
systemctl enable --now wcashexplorer-web.service
systemctl reload nginx
curl --fail --header 'Host: testnet.wcashexplorer.com' http://127.0.0.1/healthz
curl --fail --header 'Host: testnet.wcashexplorer.com' \
  http://127.0.0.1/api/v1/status
```

Before enabling public DNS, install a valid origin certificate, verify HTTPS
end to end, and use Cloudflare's **Full (strict)** SSL mode so the edge validates
that certificate. Keep DNS away from the origin until the origin checks, live
API gate, and browser QA all pass. Add HSTS only after HTTPS is verified end to
end; never announce the plain-HTTP origin as the public explorer URL.

For a dedicated host administered only by SSH keys, review and install
`deploy/ssh/00-wcashexplorer-hardening.conf` under `/etc/ssh/sshd_config.d/`.
Run `sshd -t` and confirm a fresh key-authenticated session before reloading
SSH. Keep the existing session open until that check succeeds.
