# NU7 AuxPoW fixtures

Copied unchanged from `w-cash/wolf` revision
`9ea02e480d1583dfb26ef2ad46ee3ee7e84d1f8a`,
`wcash-zcash-aux/test-vectors/nu7/`.

These contain solved Equihash (200,9) witnesses with NU7 v5/v6 parent
coinbases. Their child-chain context is synthetic: they demonstrate proof
verification and height policy, not canonical WEC/ZEC acceptance, rewards,
or parent-chain observation. Tests assign child heights around WEC 51,350
to exercise the explorer’s production verifier and rollback behavior.

SHA-256:

- `nu7-v5-wec-only.json`: `080fd622662fd3b264b979373eeef65c8f6ff006b4c9b79e453666051f12f841`
- `nu7-v6-wec-only.json`: `88be86fb92ac79587ac81499b2be6b52e6ece51201e8fa68542514174c743748`
