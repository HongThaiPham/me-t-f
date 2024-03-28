use std::ops::Mul;

use crate::{
    constants::{PERSON_SEED, TOKEN_LIMIT_AMOUNT},
    state::{InitPersonTokenParams, Person},
};
use anchor_lang::{prelude::*, system_program::create_account};
use anchor_lang::{solana_program::sysvar::rent::ID as RENT_ID, system_program::CreateAccount};
use anchor_spl::{
    associated_token::{create, AssociatedToken, Create},
    token_2022::{
        initialize_mint2,
        spl_token_2022::{
            self,
            extension::{
                metadata_pointer::instruction::initialize as initialize_metadata_pointer,
                transfer_hook::instruction::initialize as intialize_transfer_hook, ExtensionType,
            },
            instruction::AuthorityType,
            state::Mint,
        },
        InitializeMint2, Token2022,
    },
    token_interface::{mint_to, set_authority, MintTo, SetAuthority},
};
use solana_program::program::{invoke, invoke_signed};
use spl_token_metadata_interface::{
    instruction::initialize as initialize_metadata_account, state::TokenMetadata,
};

#[derive(Accounts)]
pub struct InitPersonToken<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(mut)]
    pub mint: Signer<'info>,
    #[account(init,
        payer = signer,
        space = Person::INIT_SPACE,
        seeds = [PERSON_SEED.as_ref(), signer.key().as_ref()],
        bump
    )]
    pub person: Account<'info, Person>,
    #[account(
        mut,
        seeds = [
            person.key().as_ref(),
            token_2022_program.key().as_ref(),
            mint.key().as_ref()
        ],
        seeds::program = associated_token_program.key(),
        bump
    )]
    /// CHECK: it is fine to use vault as the associated token account.
    pub vault: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK: this is fine since we are hard coding the rent sysvar.
    #[account(address = RENT_ID)]
    pub rent: UncheckedAccount<'info>,
}

impl<'info> InitPersonToken<'info> {
    pub fn init_token_mint(
        &mut self,
        params: InitPersonTokenParams,
        bumps: &InitPersonTokenBumps,
    ) -> Result<()> {
        let size = ExtensionType::try_calculate_account_len::<Mint>(&[
            ExtensionType::MetadataPointer,
            ExtensionType::TransferHook,
        ])
        .unwrap();

        let metadata = TokenMetadata {
            update_authority: spl_pod::optional_keys::OptionalNonZeroPubkey::try_from(Some(
                self.signer.key(),
            ))
            .unwrap(),
            mint: self.mint.key(),
            name: params.name,
            symbol: params.symbol,
            uri: params.uri,
            additional_metadata: vec![
                ("issuer".to_string(), "metf".to_string()),
                ("version".to_string(), "1".to_string()),
            ],
        };

        let extension_extra_space = metadata.tlv_size_of().unwrap();
        let rent = &Rent::from_account_info(&self.rent.to_account_info())?;
        let lamports = rent.minimum_balance(size + extension_extra_space);

        create_account(
            CpiContext::new(
                self.token_2022_program.to_account_info(),
                CreateAccount {
                    from: self.signer.to_account_info(),
                    to: self.mint.to_account_info(),
                },
            ),
            lamports,
            (size).try_into().unwrap(),
            &spl_token_2022::id(),
        )?;

        // add stransfer hook and metadata pointer
        invoke(
            &intialize_transfer_hook(
                &self.token_2022_program.key(),
                &self.mint.key(),
                Some(self.person.key()),
                None, // Some(*ctx.program_id),
            )?,
            &vec![self.mint.to_account_info()],
        )?;

        invoke(
            &initialize_metadata_pointer(
                &self.token_2022_program.key(),
                &self.mint.key(),
                Some(self.person.key()),
                Some(self.mint.key()),
            )?,
            &vec![self.mint.to_account_info()],
        )?;

        // initialize mint
        initialize_mint2(
            CpiContext::new(
                self.token_2022_program.to_account_info(),
                InitializeMint2 {
                    mint: self.mint.to_account_info(),
                },
            ),
            6,
            &self.signer.to_account_info().key(),
            None,
        )?;

        // init metadata account
        let seeds = &[
            PERSON_SEED.as_ref(),
            self.mint.to_account_info().key.as_ref(),
            &[bumps.person],
        ];
        let signer_seeds = &[&seeds[..]];

        invoke_signed(
            &initialize_metadata_account(
                &self.token_2022_program.key(),
                &self.mint.key(),
                &self.person.key(),
                &self.mint.key(),
                &self.signer.key(),
                metadata.name,
                metadata.symbol,
                metadata.uri,
            ),
            &vec![
                self.mint.to_account_info(),
                self.person.to_account_info(),
                self.signer.to_account_info(),
            ],
            signer_seeds,
        )?;

        // create associated token account
        create(CpiContext::new(
            self.associated_token_program.to_account_info(),
            Create {
                payer: self.signer.to_account_info(), // payer
                associated_token: self.vault.to_account_info(),
                authority: self.person.to_account_info(), // owner
                mint: self.mint.to_account_info(),
                system_program: self.system_program.to_account_info(),
                token_program: self.token_2022_program.to_account_info(),
            },
        ))?;

        // mint token to vault
        mint_to(
            CpiContext::new(
                self.token_2022_program.to_account_info(),
                MintTo {
                    mint: self.mint.to_account_info(),
                    to: self.vault.to_account_info(),
                    authority: self.signer.to_account_info(),
                },
            ),
            TOKEN_LIMIT_AMOUNT.mul(10u64.pow(9)),
        )?;

        set_authority(
            CpiContext::new(
                self.token_2022_program.to_account_info(),
                SetAuthority {
                    current_authority: self.signer.to_account_info().clone(),
                    account_or_mint: self.mint.to_account_info().clone(),
                },
            ),
            AuthorityType::MintTokens,
            None, // Set mint authority to be None
        )?;

        Ok(())
    }

    pub fn initialize(&mut self, bumps: &InitPersonTokenBumps) -> Result<()> {
        self.person.init(
            *self.signer.to_account_info().key,
            *self.mint.to_account_info().key,
            *self.mint.to_account_info().key,
            bumps.person,
        );
        Ok(())
    }
}