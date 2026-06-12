# Giveaways Production Release Checklist

Use this checklist for every mainnet giveaways deploy or upgrade. The public
repository is the source-verification input; do not rely on private workspace
state for production builds.

## Required Evidence

- Public repo commit SHA that contains the exact build inputs.
- Production SBF artifact built without `allow-early-upload`.
- Verified build log from `solana-verify build`.
- Deployed program id, programdata address, deployment slot, and deploy
  signature.
- Verified source result from `solana-verify verify-from-repo`.
- Any required The Ark database migrations applied before the upgraded program
  is activated.
- On-chain upgrade authority pubkey after deployment.
- The Ark deployment manifest and runtime env snapshot refreshed from the
  deployed program metadata.
- Public protocol upgrade notice containing commit SHA, program id, deployment
  slot, verification result, and summary of instruction/account/event changes.

## Pre-Deploy

1. Confirm the private-to-public sync is clean from the monorepo root:

   ```bash
   make giveaways-contracts.check-sync-from-private
   ```

2. Confirm public build inputs are committed:

   ```bash
   git status --short -- Cargo.toml Cargo.lock rust-toolchain.toml programs vendor
   ```

   The command must print nothing.

3. Run production and test-feature validation from this public repo:

   ```bash
   cargo fmt --all --check
   cargo build-sbf --manifest-path programs/giveaways/Cargo.toml
   cargo build-sbf --manifest-path programs/giveaways/Cargo.toml -- --features allow-early-upload -p giveaways
   cargo test --workspace --features allow-early-upload
   ```

4. Run the verified-build prerequisite check from the monorepo root:

   ```bash
   make giveaways-contracts.check-verified-deploy-prereqs ENV=mainnet
   ```

5. If this release changes account layouts or indexed event counter widths,
   deploy the matching The Ark migration before users rely on the upgraded
   program. The wide-counter giveaway ABI requires
   `the-ark-pg/db/migrations/0013_giveaway_wide_counters.sql`.

## Deploy

Prefer the verified deploy target:

```bash
make giveaways-contracts.deploy-verified \
  ENV=mainnet \
  POC_VERIFY_REPO_URL=https://github.com/proofofchance/giveaways-solana-contracts \
  POC_VERIFY_COMMIT_HASH=<public-commit-sha>
```

The mainnet target must refuse builds that include testing features and must
verify the upgrade authority immediately after deployment.

## Post-Deploy

1. Refresh The Ark program deployment metadata and runtime snapshot:

   ```bash
   ./scripts/poc-env.sh production ark -- make -e -C the-ark-pg deployment-manifest-refresh
   ./scripts/poc-env.sh production ark -- make -e -C the-ark-pg deployment-runtime-env-refresh
   ./scripts/poc-verify-enabled-deployment-manifest.sh production
   ```

2. Publish or update the protocol upgrade notice through The Ark admin API before
   users rely on the new program version. The final notice must include the
   public commit SHA, deployment slot, deploy signature, programdata address,
   source verification result, and user-visible protocol changes.

3. Verify the deployed backend after The Ark is updated. Confirm indexer status
   reports the refreshed deployment slot for `giveaways`.

## Contract Invariants

- Participation has no product-level participant cap.
- Winner finalization is chunked through `FinalizationLedger`.
- The final chunk requires `processed_count == provider_uploaded_count`.
- Participant finalization inclusion is versioned to prevent duplicate chunk
  inclusion.
- Winner selection uses deterministic top-K ranking from the final reveal seed.
- Service fee is snapshotted on each giveaway at creation.
- Zero-winner/no-attester/no-participant paths refund the creator from the vault.
- Giveaway accounts include an explicit layout version and reserved tail bytes
  before the first production deployment.
- Production builds omit the early-upload feature.
