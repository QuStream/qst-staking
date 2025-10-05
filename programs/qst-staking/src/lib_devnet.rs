use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint};

declare_id!("DEVNET_PROGRAM_ID_TO_BE_GENERATED");

// === DEVNET TESTING ECONOMICS (9 decimals for easy testing) ===
const MINIMUM_STAKE_AMOUNT: u64 = 2_000_000; // 0.002 QST (9 decimals)
const MAXIMUM_STAKE_AMOUNT: u64 = 10_000_000_000; // 10 QST (9 decimals)
const KEYS_PER_STAKE: u32 = 2;

// === DEV WALLET FOR DUST COLLECTION & DEPLOYMENT ===
const DEV_WALLET: &str = "oejJbosh9dQKKVNNPEkDZxkiTNMkMjAKjYftMGQA2ww";

// === TIME CONSTANTS (SHORTENED FOR TESTING) ===
const PRINCIPAL_LOCK_PERIOD: i64 = 60; // 1 minute for testing (represents 25 days in mainnet)
const BONUS_LOCK_PERIOD: i64 = 30;     // +30 seconds bonus
const STAKE_WINDOW_PERIOD: i64 = 300;  // 5 minutes stake window
const BONUS_ENROLLMENT_PERIOD: i64 = 120; // 2 minutes enrollment window
const EARLY_UNSTAKE_THRESHOLD_1: i64 = 14; // 14 seconds (represents 7 days in mainnet)
const EARLY_UNSTAKE_THRESHOLD_2: i64 = 30; // 30 seconds (represents 15 days in mainnet)

// === PENALTIES ===
const PENALTY_RATE_EARLY: u64 = 30; // 30% penalty for 30—15 seconds remaining
const PENALTY_RATE_LATE: u64 = 20;  // 20% penalty for 14—1 seconds remaining

#[program]
pub mod qst_staking_devnet {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, admin_wallet: Pubkey) -> Result<()> {
        // SECURITY: Only allow dev wallet to initialize AND admin must be the deployer
        let dev_wallet_key = DEV_WALLET.parse::<Pubkey>().map_err(|_| ErrorCode::InvalidDevWallet)?;
        require!(ctx.accounts.payer.key() == dev_wallet_key, ErrorCode::Unauthorized);
        require!(admin_wallet == ctx.accounts.payer.key(), ErrorCode::Unauthorized);

        // DEVNET: Allow any decimal precision for testing
        // Remove requirement: require!(ctx.accounts.qst_mint.decimals == 6, ErrorCode::InvalidMintDecimals);

        let staking_pool = &mut ctx.accounts.staking_pool;
        staking_pool.authority = admin_wallet;
        staking_pool.total_staked = 0;
        staking_pool.total_enrolled_stake = 0;
        staking_pool.penalty_vault_amount = 0;
        staking_pool.first_stake_timestamp = 0;
        staking_pool.bonus_enrollment_deadline = 0;
        staking_pool.stake_window_end = 0;
        staking_pool.qst_mint = ctx.accounts.qst_mint.key();
        staking_pool.bump = ctx.bumps.staking_pool;

        msg!("QST Staking Pool (DEVNET) initialized with admin: {:?}", admin_wallet);
        Ok(())
    }

    pub fn start_stake_window(ctx: Context<StartStakeWindow>) -> Result<()> {
        let staking_pool = &mut ctx.accounts.staking_pool;

        // DEVNET: Relaxed access control for testing - anyone can start window
        // TODO: Restore admin check for mainnet
        // require!(ctx.accounts.admin.key() == staking_pool.authority, ErrorCode::Unauthorized);

        let current_time = Clock::get()?.unix_timestamp;
        staking_pool.first_stake_timestamp = current_time;
        staking_pool.stake_window_end = current_time + STAKE_WINDOW_PERIOD;
        staking_pool.bonus_enrollment_deadline = current_time + BONUS_ENROLLMENT_PERIOD;

        emit!(StakeWindowStarted {
            start_time: current_time,
            stake_window_end: staking_pool.stake_window_end,
            bonus_enrollment_deadline: staking_pool.bonus_enrollment_deadline,
        });

        msg!(
            "DEVNET Stake window started. Window ends: {}, Bonus enrollment deadline: {}",
            staking_pool.stake_window_end,
            staking_pool.bonus_enrollment_deadline
        );
        Ok(())
    }

    pub fn enroll_in_bonus(ctx: Context<EnrollInBonus>) -> Result<()> {
        let staking_pool = &mut ctx.accounts.staking_pool;
        let stake_account = &mut ctx.accounts.stake_account;

        let current_time = Clock::get()?.unix_timestamp;

        // Check if bonus enrollment is still open
        require!(
            current_time <= staking_pool.bonus_enrollment_deadline,
            ErrorCode::BonusEnrollmentClosed
        );
        require!(
            staking_pool.bonus_enrollment_deadline > 0,
            ErrorCode::StakeWindowNotStarted
        );

        // User must have existing stake to enroll
        require!(stake_account.amount > 0, ErrorCode::NoStakeToEnroll);
        require!(!stake_account.enrolled_in_bonus, ErrorCode::AlreadyEnrolledInBonus);

        // Enroll
        stake_account.enrolled_in_bonus = true;

        // On enroll, add the user's current principal to total_enrolled_stake
        staking_pool.total_enrolled_stake = staking_pool
            .total_enrolled_stake
            .checked_add(stake_account.amount)
            .ok_or(ErrorCode::NumericOverflow)?;

        // Set bonus unlock to principal + bonus period
        stake_account.bonus_unlock_time = stake_account.principal_unlock_time + BONUS_LOCK_PERIOD;

        emit!(BonusEnrollment {
            user: ctx.accounts.user.key(),
            enrolled_stake: stake_account.amount,
            timestamp: current_time,
        });

        msg!(
            "DEVNET: User {} enrolled in bonus with stake: {}",
            ctx.accounts.user.key(),
            stake_account.amount
        );
        Ok(())
    }

    pub fn stake_tokens(ctx: Context<StakeTokens>, amount: u64) -> Result<()> {
        msg!("=== DEVNET STAKE_TOKENS ===");
        msg!("Amount to stake: {}", amount);

        let staking_pool = &mut ctx.accounts.staking_pool;
        let stake_account = &mut ctx.accounts.stake_account;
        let user_token_account = &ctx.accounts.user_token_account;
        let pool_token_account = &ctx.accounts.pool_token_account;

        // DEVNET: Validate amount (smaller minimums for testing)
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(amount >= MINIMUM_STAKE_AMOUNT, ErrorCode::InsufficientStakeAmount);
        require!(amount <= MAXIMUM_STAKE_AMOUNT, ErrorCode::StakeAmountTooLarge);
        require!(amount % MINIMUM_STAKE_AMOUNT == 0, ErrorCode::InvalidStakeAmount);
        msg!("✅ DEVNET Amount checks passed");

        let current_time = Clock::get()?.unix_timestamp;

        // DEVNET: Auto-start window for easy testing
        if staking_pool.first_stake_timestamp == 0 || current_time > staking_pool.stake_window_end {
            msg!("🔄 DEVNET: Auto-starting stake window");
            staking_pool.first_stake_timestamp = current_time;
            staking_pool.stake_window_end = current_time + STAKE_WINDOW_PERIOD;
            staking_pool.bonus_enrollment_deadline = current_time + BONUS_ENROLLMENT_PERIOD;
        }

        // Calculate number of keys based on stake amount
        let stake_multiplier = amount / MINIMUM_STAKE_AMOUNT;
        let raw_keys = stake_multiplier
            .checked_mul(KEYS_PER_STAKE as u64)
            .ok_or(ErrorCode::NumericOverflow)?;
        let node_keys_earned = u32::try_from(raw_keys).map_err(|_| ErrorCode::NumericOverflow)?;
        
        msg!("DEVNET Node keys to earn: {}", node_keys_earned);

        // Transfer tokens from user to pool
        let cpi_accounts = Transfer {
            from: user_token_account.to_account_info(),
            to: pool_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        // Initialize or update stake account
        if stake_account.amount == 0 {
            msg!("🆕 DEVNET: Initializing new stake account");
            stake_account.user = ctx.accounts.user.key();
            stake_account.amount = amount;
            stake_account.node_keys_earned = node_keys_earned;
            stake_account.principal_unlock_time = current_time + PRINCIPAL_LOCK_PERIOD;
            stake_account.bonus_unlock_time = 0;
            stake_account.enrolled_in_bonus = false;
            stake_account.bump = ctx.bumps.stake_account;
        } else {
            msg!("📈 DEVNET: Updating existing stake account");
            stake_account.amount = stake_account
                .amount
                .checked_add(amount)
                .ok_or(ErrorCode::NumericOverflow)?;
            stake_account.node_keys_earned = stake_account
                .node_keys_earned
                .checked_add(node_keys_earned)
                .ok_or(ErrorCode::NumericOverflow)?;
            stake_account.principal_unlock_time = current_time + PRINCIPAL_LOCK_PERIOD;

            if stake_account.enrolled_in_bonus {
                stake_account.bonus_unlock_time = stake_account.principal_unlock_time + BONUS_LOCK_PERIOD;
            }
        }

        // Update pool totals
        staking_pool.total_staked = staking_pool
            .total_staked
            .checked_add(amount)
            .ok_or(ErrorCode::NumericOverflow)?;

        if stake_account.enrolled_in_bonus {
            staking_pool.total_enrolled_stake = staking_pool
                .total_enrolled_stake
                .checked_add(amount)
                .ok_or(ErrorCode::NumericOverflow)?;
        }

        emit!(StakeEvent {
            user: ctx.accounts.user.key(),
            amount,
            total_staked: staking_pool.total_staked,
            node_keys_earned,
            principal_unlock_time: stake_account.principal_unlock_time,
            bonus_unlock_time: stake_account.bonus_unlock_time,
            enrolled_in_bonus: stake_account.enrolled_in_bonus,
            timestamp: current_time,
        });

        msg!("✅ DEVNET STAKE_TOKENS COMPLETED");
        Ok(())
    }

    pub fn unstake_tokens(ctx: Context<UnstakeTokens>, amount: u64) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake_account;
        let staking_pool = &mut ctx.accounts.staking_pool;

        require!(stake_account.amount >= amount, ErrorCode::InsufficientStakeBalance);
        require!(!stake_account.enrolled_in_bonus, ErrorCode::BonusEnrolledCannotUnstake);

        let current_time = Clock::get()?.unix_timestamp;
        let time_until_unlock = stake_account.principal_unlock_time - current_time;

        // DEVNET: Shortened time thresholds for testing
        let (penalty_amount, net_amount) = if time_until_unlock <= 0 {
            (0u64, amount)
        } else if time_until_unlock <= EARLY_UNSTAKE_THRESHOLD_1 {
            let penalty = ((amount as u128) * (PENALTY_RATE_LATE as u128) / 100u128) as u64;
            (penalty, amount - penalty)
        } else if time_until_unlock <= EARLY_UNSTAKE_THRESHOLD_2 {
            let penalty = ((amount as u128) * (PENALTY_RATE_EARLY as u128) / 100u128) as u64;
            (penalty, amount - penalty)
        } else {
            return Err(ErrorCode::UnstakeBlocked.into());
        };

        // Setup PDA signer and transfer
        let authority_seed = b"staking_pool";
        let bump_bytes = [staking_pool.bump];
        let signer_seeds = &[authority_seed.as_ref(), bump_bytes.as_ref()];
        let signer = &[signer_seeds.as_ref()];

        if net_amount > 0 {
            let cpi_accounts = Transfer {
                from: ctx.accounts.pool_token_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: staking_pool.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, net_amount)?;
        }

        // Update state
        if penalty_amount > 0 {
            staking_pool.penalty_vault_amount = staking_pool
                .penalty_vault_amount
                .checked_add(penalty_amount)
                .ok_or(ErrorCode::NumericOverflow)?;
        }

        stake_account.amount = stake_account
            .amount
            .checked_sub(amount)
            .ok_or(ErrorCode::NumericOverflow)?;
        staking_pool.total_staked = staking_pool
            .total_staked
            .checked_sub(amount)
            .ok_or(ErrorCode::NumericOverflow)?;

        msg!("DEVNET: User {} unstaked {} tokens, penalty: {}", 
             ctx.accounts.user.key(), amount, penalty_amount);
        Ok(())
    }

    pub fn withdraw_all(ctx: Context<WithdrawAll>) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake_account;
        let staking_pool = &mut ctx.accounts.staking_pool;

        let current_time = Clock::get()?.unix_timestamp;

        // Check appropriate unlock time based on enrollment status
        let required_unlock_time = if stake_account.enrolled_in_bonus {
            stake_account.bonus_unlock_time
        } else {
            stake_account.principal_unlock_time
        };

        require!(current_time >= required_unlock_time, ErrorCode::StillLocked);
        require!(stake_account.amount > 0, ErrorCode::NoStakeToWithdraw);

        let principal_amount = stake_account.amount;
        let mut bonus_rewards = 0u64;

        // Calculate bonus rewards if enrolled and bonus period ended
        if stake_account.enrolled_in_bonus && current_time >= stake_account.bonus_unlock_time {
            if staking_pool.total_enrolled_stake > 0 && staking_pool.penalty_vault_amount > 0 {
                let pv = staking_pool.penalty_vault_amount as u128;
                let user_amt = stake_account.amount as u128;
                let total = staking_pool.total_enrolled_stake as u128;
                bonus_rewards = if total > 0 { ((pv * user_amt) / total) as u64 } else { 0 };
                
                staking_pool.penalty_vault_amount =
                    staking_pool.penalty_vault_amount.saturating_sub(bonus_rewards);
            }
        }

        let total_withdrawal = principal_amount + bonus_rewards;

        // Setup PDA signer and transfer
        let authority_seed = b"staking_pool";
        let bump_bytes = [staking_pool.bump];
        let signer_seeds = &[authority_seed.as_ref(), bump_bytes.as_ref()];
        let signer = &[signer_seeds.as_ref()];

        let cpi_accounts = Transfer {
            from: ctx.accounts.pool_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: staking_pool.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, total_withdrawal)?;

        // Update pool state
        staking_pool.total_staked = staking_pool
            .total_staked
            .checked_sub(principal_amount)
            .ok_or(ErrorCode::NumericOverflow)?;

        if stake_account.enrolled_in_bonus {
            staking_pool.total_enrolled_stake = staking_pool
                .total_enrolled_stake
                .saturating_sub(principal_amount);
        }

        // Reset stake account but keep node keys
        let node_keys_to_keep = stake_account.node_keys_earned;
        stake_account.amount = 0;
        stake_account.node_keys_earned = node_keys_to_keep;
        stake_account.enrolled_in_bonus = false;
        stake_account.principal_unlock_time = 0;
        stake_account.bonus_unlock_time = 0;

        msg!("DEVNET: User {} withdrew {} principal + {} bonus = {} total", 
             ctx.accounts.user.key(), principal_amount, bonus_rewards, total_withdrawal);
        Ok(())
    }

    pub fn get_stake_info(ctx: Context<GetStakeInfo>) -> Result<StakeInfo> {
        let stake_account = &ctx.accounts.stake_account;
        let staking_pool = &ctx.accounts.staking_pool;

        let potential_bonus = if stake_account.enrolled_in_bonus && staking_pool.total_enrolled_stake > 0 {
            let pv = staking_pool.penalty_vault_amount as u128;
            let user_amt = stake_account.amount as u128;
            let total = staking_pool.total_enrolled_stake as u128;
            if total > 0 { ((pv * user_amt) / total) as u64 } else { 0 }
        } else {
            0
        };

        let current_time = Clock::get()?.unix_timestamp;
        let unlock_time = if stake_account.enrolled_in_bonus {
            stake_account.bonus_unlock_time
        } else {
            stake_account.principal_unlock_time
        };

        Ok(StakeInfo {
            amount: stake_account.amount,
            node_keys_earned: stake_account.node_keys_earned,
            principal_unlock_time: stake_account.principal_unlock_time,
            bonus_unlock_time: stake_account.bonus_unlock_time,
            enrolled_in_bonus: stake_account.enrolled_in_bonus,
            potential_bonus,
            unlock_time,
            is_unlocked: current_time >= unlock_time,
            time_until_unlock: if current_time >= unlock_time { 0 } else { unlock_time - current_time },
        })
    }
}

// All the same account structures as mainnet version...
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 32 + 1,
        seeds = [b"staking_pool"],
        bump
    )]
    pub staking_pool: Account<'info, StakingPool>,
    #[account()]
    pub qst_mint: Account<'info, Mint>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct StartStakeWindow<'info> {
    #[account(mut, seeds = [b"staking_pool"], bump = staking_pool.bump)]
    pub staking_pool: Account<'info, StakingPool>,
    #[account()]
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct EnrollInBonus<'info> {
    #[account(mut, seeds = [b"staking_pool"], bump = staking_pool.bump)]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(mut, seeds = [b"stake_account", user.key().as_ref()], bump = stake_account.bump)]
    pub stake_account: Account<'info, StakeAccount>,
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct StakeTokens<'info> {
    #[account(mut, seeds = [b"staking_pool"], bump = staking_pool.bump)]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + 32 + 8 + 4 + 8 + 8 + 1 + 1,
        seeds = [b"stake_account", user.key().as_ref()],
        bump
    )]
    pub stake_account: Account<'info, StakeAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub pool_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UnstakeTokens<'info> {
    #[account(mut, seeds = [b"staking_pool"], bump = staking_pool.bump)]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(mut, seeds = [b"stake_account", user.key().as_ref()], bump = stake_account.bump)]
    pub stake_account: Account<'info, StakeAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub pool_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawAll<'info> {
    #[account(mut, seeds = [b"staking_pool"], bump = staking_pool.bump)]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(mut, seeds = [b"stake_account", user.key().as_ref()], bump = stake_account.bump)]
    pub stake_account: Account<'info, StakeAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub pool_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct GetStakeInfo<'info> {
    #[account(seeds = [b"staking_pool"], bump = staking_pool.bump)]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(seeds = [b"stake_account", user.key().as_ref()], bump = stake_account.bump)]
    pub stake_account: Account<'info, StakeAccount>,
    pub user: Signer<'info>,
}

#[account]
pub struct StakingPool {
    pub authority: Pubkey,
    pub total_staked: u64,
    pub total_enrolled_stake: u64,
    pub penalty_vault_amount: u64,
    pub first_stake_timestamp: i64,
    pub bonus_enrollment_deadline: i64,
    pub stake_window_end: i64,
    pub qst_mint: Pubkey,
    pub bump: u8,
}

#[account]
pub struct StakeAccount {
    pub user: Pubkey,
    pub amount: u64,
    pub node_keys_earned: u32,
    pub principal_unlock_time: i64,
    pub bonus_unlock_time: i64,
    pub enrolled_in_bonus: bool,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct StakeInfo {
    pub amount: u64,
    pub node_keys_earned: u32,
    pub principal_unlock_time: i64,
    pub bonus_unlock_time: i64,
    pub enrolled_in_bonus: bool,
    pub potential_bonus: u64,
    pub unlock_time: i64,
    pub is_unlocked: bool,
    pub time_until_unlock: i64,
}

#[event]
pub struct StakeWindowStarted {
    pub start_time: i64,
    pub stake_window_end: i64,
    pub bonus_enrollment_deadline: i64,
}

#[event]
pub struct BonusEnrollment {
    pub user: Pubkey,
    pub enrolled_stake: u64,
    pub timestamp: i64,
}

#[event]
pub struct StakeEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub total_staked: u64,
    pub node_keys_earned: u32,
    pub principal_unlock_time: i64,
    pub bonus_unlock_time: i64,
    pub enrolled_in_bonus: bool,
    pub timestamp: i64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Insufficient stake amount. Minimum is 0.002 QST")]
    InsufficientStakeAmount,
    #[msg("Stake amount too large. Maximum is 10 QST per stake")]
    StakeAmountTooLarge,
    #[msg("Invalid stake amount. Must be multiple of 0.002 QST")]
    InvalidStakeAmount,
    #[msg("Insufficient stake balance")]
    InsufficientStakeBalance,
    #[msg("Unstake blocked. More than 40 seconds remaining until unlock")]
    UnstakeBlocked,
    #[msg("Still locked. Cannot withdraw before unlock time")]
    StillLocked,
    #[msg("No stake to withdraw")]
    NoStakeToWithdraw,
    #[msg("Stake window is closed")]
    StakeWindowClosed,
    #[msg("Bonus enrollment period has closed")]
    BonusEnrollmentClosed,
    #[msg("Stake window has not been started yet")]
    StakeWindowNotStarted,
    #[msg("No stake to enroll in bonus")]
    NoStakeToEnroll,
    #[msg("Already enrolled in bonus")]
    AlreadyEnrolledInBonus,
    #[msg("Cannot unstake when enrolled in bonus. Must wait for full unlock period.")]
    BonusEnrolledCannotUnstake,
    #[msg("Invalid dev wallet address")]
    InvalidDevWallet,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Numeric overflow")]
    NumericOverflow,
}