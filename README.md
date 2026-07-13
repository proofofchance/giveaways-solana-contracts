# Giveaways Solana Contracts

This folder contains the extracted giveaways Solana program source.

Operational files live at the repo root:

- Make targets: `giveaways-solana-contracts.mk`
- scripts: `solana-scripts/giveaways/`
- secrets and env: root `.env` plus root `.secrets/`

Production release checklist:

- [PRODUCTION_RELEASE_CHECKLIST.md](PRODUCTION_RELEASE_CHECKLIST.md)

Use the root Make targets for build, deploy, and workflow commands. The main
ones are:

- `make giveaways-contracts.build`
- `make giveaways-contracts.test`
- `make giveaways-contracts.deploy ENV=devnet`
- `make giveaways-contracts.create-giveaway`
- `make giveaways-contracts.begin-giveaway-upload id=<GIVEAWAY_ID>`
- `make giveaways-contracts.upload-reveals id=<GIVEAWAY_ID>`
- `make giveaways-contracts.finalize-giveaway-winners id=<GIVEAWAY_ID>`
- `make giveaways-contracts.settle-payout-batch id=<GIVEAWAY_ID>`
- `make giveaways-contracts.close-no-participants id=<GIVEAWAY_ID>`

Notes:

- Giveaway branding and off-chain requirements stay outside the on-chain create
  flow.
- The upload/attestation phase accepts either a provider Ed25519 receipt or a
  participant-signed direct reveal. Provider receipts bind the participant
  wallet and may be relayed by any fee payer. Reveal plaintext is exactly
  `lucky_words || 0x1f || salt`.
- Winner finalization validates every supplied participant account as a program
  owned PDA derived from `["participant", giveaway, wallet]` and consumes the
  exact immutable participant index expected by `FinalizationLedger`. All
  participants are scanned; only verified, non-disqualified revealers are
  eligible. Giveaways are capped at 4,096 participants.
- Winner selection uses deterministic top-K ranking from the final reveal
  aggregate, indexed participant commitment, giveaway id, and participant
  wallet. A final scan emits every winner for immutable replay, then winner
  computation and locking complete atomically.
- Giveaway accounts include an explicit layout version and reserved bytes so
  future controlled upgrades can add fields without immediately changing the
  serialized account size.
- Giveaway creation rejects active windows outside the on-chain min/max bounds
  and rejects prize pools that cannot pay at least one lamport to every
  requested winner after the service fee snapshot.
- The authority-only `begin-giveaway-upload` fast-track is an intentional
  staging/testing feature. It lets the authority end participation early and
  start the upload phase before the configured active deadline, but only when
  the program is built with the `allow-early-upload` Cargo feature.
- Secrets are no longer stored inside this workspace.

Participant-reveal entropy remains subject to a last-revealer reveal-or-forfeit
choice. The selection is deterministic and reproducible, but is not equivalent
to an external randomness beacon.
