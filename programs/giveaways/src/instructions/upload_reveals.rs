//! # Upload Reveals Instruction
//!
//! Allows the service provider to upload batches of participant reveals
//! for proof-of-chance entropy generation.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant},
    utils::crypto::{
        build_reveal_plaintext, compute_reveal_digest, verify_reveal, xor_reveal_digests,
    },
};
use anchor_lang::prelude::*;
use std::io::Cursor;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RevealData {
    pub participant: Pubkey,
    pub lucky_words: String,
    pub salt: Vec<u8>,
}

#[derive(Accounts)]
pub struct UploadReveals<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        constraint = authority.key() == config.authority @ GiveawayError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

pub fn process(ctx: Context<UploadReveals>, reveals: Vec<RevealData>) -> Result<()> {
    let _config = &ctx.accounts.config;
    let giveaway = &mut ctx.accounts.giveaway;
    let _authority = &ctx.accounts.authority;
    let clock = Clock::get()?;

    // Validate batch size
    require!(
        !reveals.is_empty() && reveals.len() <= MAX_REVEALS_PER_BATCH,
        GiveawayError::InvalidInstruction
    );

    // Reveals can be uploaded once the upload phase has started. They may also
    // be uploaded immediately when all participants have attested.
    let upload_started = clock.unix_timestamp >= giveaway.upload_start_unix;
    let all_attested =
        giveaway.participants_count > 0 && giveaway.attested_count == giveaway.participants_count;
    require!(
        upload_started || all_attested,
        GiveawayError::UploadWindowNotStarted
    );
    require!(!giveaway.winners_computed, GiveawayError::WinnersLocked);

    let mut valid_reveals = 0u32;
    let mut reveal_digests = Vec::with_capacity(reveals.len());
    let mut seen = std::collections::HashSet::new();

    // Process each reveal in the batch
    for reveal in reveals.iter() {
        require!(
            seen.insert(reveal.participant),
            GiveawayError::InvalidReveal
        );

        // Validate reveal data lengths
        require!(
            reveal.lucky_words.len() <= MAX_LUCKY_WORDS_LEN && reveal.salt.len() <= MAX_SALT_LEN,
            GiveawayError::TextTooLong
        );

        // Get participant account from remaining accounts
        let participant_account_info = ctx
            .remaining_accounts
            .iter()
            .find(|acc| acc.key() == reveal.participant);

        let participant_info = participant_account_info.ok_or(GiveawayError::InvalidAccount)?;
        require!(
            participant_info.is_writable,
            GiveawayError::AccountNotWritable
        );

        // Deserialize participant account
        let mut participant_data = &participant_info.data.borrow()[..];
        let mut participant = Participant::try_deserialize(&mut participant_data)
            .map_err(|_| GiveawayError::AccountNotInitialized)?;

        require_keys_eq!(
            participant.giveaway,
            giveaway.key(),
            GiveawayError::InvalidAccount
        );
        require!(
            !participant.reveal_included,
            GiveawayError::RevealAlreadyIncluded
        );
        require!(
            !participant.disqualified,
            GiveawayError::AlreadyDisqualified
        );
        require!(participant.attested_uploaded, GiveawayError::NotAttested);

        // Verify reveal against commitment
        require!(
            verify_reveal(
                &participant.commitment_hash,
                &reveal.lucky_words,
                &reveal.salt,
            )?,
            GiveawayError::InvalidReveal
        );

        let plaintext = build_reveal_plaintext(&reveal.lucky_words, &reveal.salt)?;
        reveal_digests.push(compute_reveal_digest(&participant.wallet, &plaintext));

        // Mark reveal as included and write back.
        participant.mark_reveal_included(clock.unix_timestamp);

        let mut account_data = participant_info.data.borrow_mut();
        account_data.fill(0);
        let mut writer = Cursor::new(&mut account_data[..]);
        participant.try_serialize(&mut writer)?;

        valid_reveals += 1;
    }

    // Update giveaway reveal count
    let aggregate_hash = xor_reveal_digests(giveaway.poc_aggregate_hash, &reveal_digests);
    giveaway.add_uploaded_reveals(valid_reveals, aggregate_hash);

    // Emit event
    crate::events::GiveawayEvent::RevealsUploaded {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        batch_size: valid_reveals,
        total_reveals_uploaded: giveaway.provider_uploaded_count,
        total_attested: giveaway.attested_count,
        aggregate_hash: hex::encode(aggregate_hash),
        uploads_complete: giveaway.uploads_complete,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Uploaded {} valid reveals for giveaway {} (total: {})",
        valid_reveals,
        giveaway.id,
        giveaway.provider_uploaded_count
    );

    Ok(())
}
