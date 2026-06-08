# Giveaways Solana Contracts

This folder contains the extracted giveaways Solana program source.

Common local commands:

- `cargo fmt --all --check`
- `cargo test --workspace`
- `npm test`
- `npm run lint`

Notes:

- Giveaway branding and off-chain requirements stay outside the on-chain create
  flow.
- The first production protocol uses an upload/attestation phase. Participant
  attestations require a provider Ed25519 receipt, and reveal plaintext is
  `lucky_words || 0x1f || salt`.
