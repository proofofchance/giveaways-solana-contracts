# Giveaways Solana Contracts

This folder contains the extracted giveaways Solana program source.

Operational files live at the repo root:

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
- Winner finalization validates every supplied participant account as a program
  owned PDA derived from `["participant", giveaway, wallet]`, rejects duplicate
  account keys or wallets, and only computes winners from the canonical
  reveal-included set.
- Giveaway creation rejects active windows outside the on-chain min/max bounds
  and rejects prize pools that cannot pay at least one lamport to every
  requested winner after the service fee snapshot.
- The authority-only `begin-giveaway-upload` fast-track is an intentional
  staging/testing feature. It lets the authority end participation early and
  start the upload phase before the configured active deadline, but only when
  the program is built with the `allow-early-upload` Cargo feature.
- Secrets are no longer stored inside this workspace.
