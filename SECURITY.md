# Security Policy

WcashExplorer processes consensus-adjacent data and displays it to users, but it
is not a wallet, signer, custody service, mining pool, or consensus node. Treat
all explorer output as informational and verify critical claims with your own
nodes.

## Supported versions

The project is pre-release. Security fixes are applied to the current default
branch only. There is no supported public deployment or mainnet release yet.

## Reporting a vulnerability

Please use GitHub's **Report a vulnerability** flow in the repository Security
tab to open a private security advisory. Do not file a public issue for a
suspected vulnerability.

Include, when possible:

- the affected revision and component;
- the impact and conditions required to reproduce it;
- a minimal reproduction or test case;
- whether the issue could expose RPC credentials, corrupt canonical selection,
  misstate AuxPoW evidence, or reveal data beyond the public chain view; and
- a safe way to contact you for follow-up.

Do not include live credentials, private keys, wallet data, production database
dumps, or private infrastructure addresses. Replace them with clearly marked
test values.

Maintainers will acknowledge a complete report, assess severity, coordinate a
fix, and agree on disclosure timing through the private advisory. No bounty is
promised by this policy.

## High-impact areas

Reports are especially valuable when they concern:

- acceptance or display of invalid AuxPoW proofs;
- confusion between local proof validity, Wcash witness state, and Zcash parent
  observations;
- reorganization handling or non-atomic canonical-chain updates;
- transaction-instance confusion between `txid` and authorization digest;
- disclosure of node RPC authentication, database credentials, or log secrets;
- request amplification, unbounded node responses, or denial of service;
- injection, cross-site scripting, or unsafe external links; or
- claims that infer hidden shielded parties, values, balances, or memos.

## Operator responsibilities

Keep Wcash, Zcash, and PostgreSQL listeners on private networks. Terminate TLS at
a hardened reverse proxy, restrict outbound RPC access, rotate credentials, and
monitor `/health/ready`. Configure parent observations from operationally
separate sources where possible. Distinct endpoint URLs alone do not prove
independent operators.
