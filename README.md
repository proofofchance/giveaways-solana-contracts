# Giveaways Solana Contracts

This folder contains the extracted giveaways Solana program source.

Operational files live at the repo root:

- requirements: `GIVEAWAYS_SOLANA_CONTRACTS_REQUIREMENTS.md`
- Make targets: `giveaways-solana-contracts.mk`
- scripts: `solana-scripts/giveaways/`
- secrets and env: root `.env` plus root `.secrets/`

Use the root Make targets for build, deploy, and workflow commands. The main
ones are:

- `make giveaways-contracts.build`
- `make giveaways-contracts.test`
- `make giveaways-contracts.deploy ENV=devnet`
- `make giveaways-contracts.create-giveaway`
- `make giveaways-contracts.begin-giveaway-upload id=<GIVEAWAY_ID>`
- `make giveaways-contracts.upload-reveals id=<GIVEAWAY_ID>`
- `make giveaways-contracts.finalize-giveaway-winners id=<GIVEAWAY_ID>`
- `make giveaways-contracts.lock-winners id=<GIVEAWAY_ID>`
- `make giveaways-contracts.settle-payout-batch id=<GIVEAWAY_ID>`
- `make giveaways-contracts.close-no-participants id=<GIVEAWAY_ID>`

Notes:

- Giveaway branding and off-chain requirements stay outside the on-chain create
  flow.
- The first production protocol uses an upload/attestation phase. Participant
  attestations require a provider Ed25519 receipt, and reveal plaintext is
  `lucky_words || 0x1f || salt`.
- Secrets are no longer stored inside this workspace.
