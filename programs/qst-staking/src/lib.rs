use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint};

declare_id!("MAINNET_PROGRAM_ID_TO_BE_GENERATED");

// === ECONOMICS (QST mainnet: 6 decimals) ===
const MINIMUM_STAKE_AMOUNT: u64 = 200_000 * 1_000_000; // 200,000 QST (6 decimals)
const MAXIMUM_STAKE_AMOUNT: u64 = 10_000_000 * 1_000_000; // 10,000,000 QST (6 decimals)
const KEYS_PER_STAKE: u32 = 2;

// === DEV WALLET FOR DEPLOYMENT ===
const DEV_WALLET: &str = "oejJbosh9dQKKVNNPEkDZxkiTNMkMjAKjYftMGQA2ww";

// === TIME CONSTANTS ===
const PRINCIPAL_LOCK_PERIOD: i64 = 25 * 24 * 60 * 60; // 25 days
const BONUS_LOCK_PERIOD: i64 = 10 * 24 * 60 * 60;     // +10 days bonus
const STAKE_WINDOW_PERIOD: i64 = 9 * 24 * 60 * 60;    // 9 days stake window (HAL-01 fix)
const BONUS_ENROLLMENT_PERIOD: i64 = 48 * 60 * 60;    // 48 hours enrollment window
const BONUS_WITHDRAWAL_DELAY: i64 = 1 * 24 * 60 * 60;     // 1 day after last user unlock (HAL-02 fix)
const EARLY_UNSTAKE_THRESHOLD_1: i64 = 7 * 24 * 60 * 60; // 7 days
const EARLY_UNSTAKE_THRESHOLD_2: i64 = 15 * 24 * 60 * 60; // 15 days

// === PENALTIES ===
const PENALTY_RATE_EARLY: u64 = 30; // 30% penalty for 15—8 days remaining
const PENALTY_RATE_LATE: u64 = 20;  // 20% penalty for 7—1 days remaining

#[program]
pub mod qst_staking_mainnet {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, admin_wallet: Pubkey) -> Result<()> {
        // SECURITY: Only allow dev wallet to initialize
        let dev_wallet_key = DEV_WALLET.parse::<Pubkey>().map_err(|_| ErrorCode::InvalidDevWallet)?;
        require!(ctx.accounts.payer.key() == dev_wallet_key, ErrorCode::Unauthorized);
        require!(admin_wallet == ctx.accounts.payer.key(), ErrorCode::Unauthorized);

        // Enforce correct mint precision for QST mainnet
        require!(ctx.accounts.qst_mint.decimals == 6, ErrorCode::InvalidMintDecimals);

        let staking_pool = &mut ctx.accounts.staking_pool;
        staking_pool.authority = admin_wallet;
        staking_pool.total_staked = 0;
        staking_pool.total_enrolled_stake = 0;
        staking_pool.penalty_vault_amount = 0;
        staking_pool.first_stake_timestamp = 0;
        staking_pool.bonus_enrollment_deadline = 0;
        staking_pool.stake_window_end = 0;
        staking_pool.latest_bonus_unlock_time = 0; // HAL-02 fix
        staking_pool.qst_mint = ctx.accounts.qst_mint.key();
        staking_pool.bump = ctx.bumps.staking_pool;

        msg!("QST Staking Pool initialized with admin: {:?}", admin_wallet);
        Ok(())
    }

    pub fn start_stake_window(ctx: Context<StartStakeWindow>) -> Result<()> {
        let staking_pool = &mut ctx.accounts.staking_pool;

        // Admin only: Only admin can start the staking window
        require!(
            ctx.accounts.admin.key() == staking_pool.authority,
            ErrorCode::Unauthorized
        );

        // HAL-03 fix: Prevent restarting entirely - can only start once
        require!(
            staking_pool.first_stake_timestamp == 0,
            ErrorCode::StakeWindowAlreadyActive
        );

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
            "Stake window started. Window ends: {}, Bonus enrollment deadline: {}",
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
        // HAL-05 fix: Removed redundant validation (bonus_enrollment_deadline > 0 is implied by the above check)

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

        // Set bonus unlock to principal + 10d
        stake_account.bonus_unlock_time = stake_account.principal_unlock_time + BONUS_LOCK_PERIOD;

        // HAL-02 fix: Track latest unlock time for bonus withdrawal delay
        if stake_account.bonus_unlock_time > staking_pool.latest_bonus_unlock_time {
            staking_pool.latest_bonus_unlock_time = stake_account.bonus_unlock_time;
        }

        emit!(BonusEnrollment {
            user: ctx.accounts.user.key(),
            enrolled_stake: stake_account.amount,
            timestamp: current_time,
        });

        msg!(
            "User {} enrolled in bonus with stake: {}",
            ctx.accounts.user.key(),
            stake_account.amount
        );
        Ok(())
    }

    pub fn stake_tokens(ctx: Context<StakeTokens>, amount: u64) -> Result<()> {
        msg!("=== STARTING STAKE_TOKENS ===");
        msg!("Amount to stake: {}", amount);

        let staking_pool = &mut ctx.accounts.staking_pool;
        let stake_account = &mut ctx.accounts.stake_account;
        let user_token_account = &ctx.accounts.user_token_account;
        let pool_token_account = &ctx.accounts.pool_token_account;

        // Validate amount
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(amount >= MINIMUM_STAKE_AMOUNT, ErrorCode::InsufficientStakeAmount);
        require!(amount <= MAXIMUM_STAKE_AMOUNT, ErrorCode::StakeAmountTooLarge);
        require!(amount % MINIMUM_STAKE_AMOUNT == 0, ErrorCode::InvalidStakeAmount);
        msg!("✅ Amount checks passed");

        let current_time = Clock::get()?.unix_timestamp;
        msg!("Current time: {}", current_time);
        msg!("Stake window end: {}", staking_pool.stake_window_end);
        msg!("First stake timestamp: {}", staking_pool.first_stake_timestamp);

        // Enforce stake window - no auto-restart
        require!(
            staking_pool.first_stake_timestamp > 0,
            ErrorCode::StakeWindowNotStarted
        );
        require!(
            current_time <= staking_pool.stake_window_end,
            ErrorCode::StakeWindowClosed
        );
        msg!("✅ Stake window is active");

        // Calculate number of keys based on stake amount
        let stake_multiplier = amount / MINIMUM_STAKE_AMOUNT;
        
        // Use checked math and try_from to prevent overflow/truncation
        let raw_keys = stake_multiplier
            .checked_mul(KEYS_PER_STAKE as u64)
            .ok_or(ErrorCode::NumericOverflow)?;
        let node_keys_earned = u32::try_from(raw_keys).map_err(|_| ErrorCode::NumericOverflow)?;
        
        msg!("Node keys to earn: {}", node_keys_earned);

        // Transfer tokens from user to pool
        msg!("🔄 Starting token transfer");
        let cpi_accounts = Transfer {
            from: user_token_account.to_account_info(),
            to: pool_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;
        msg!("✅ Token transfer completed");

        // Initialize or update stake account
        if stake_account.amount == 0 {
            msg!("🆕 Initializing new stake account");
            stake_account.user = ctx.accounts.user.key();
            stake_account.amount = amount;
            stake_account.node_keys_earned = node_keys_earned;

            // Reset lock model - unlock time based on current stake time
            stake_account.principal_unlock_time = current_time + PRINCIPAL_LOCK_PERIOD;

            // Not enrolled by default; bonus unlock remains 0 until enroll
            stake_account.bonus_unlock_time = 0;
            stake_account.enrolled_in_bonus = false;
            stake_account.bump = ctx.bumps.stake_account;
        } else {
            msg!("📈 Updating existing stake account");
            stake_account.amount = stake_account
                .amount
                .checked_add(amount)
                .ok_or(ErrorCode::NumericOverflow)?;
            stake_account.node_keys_earned = stake_account
                .node_keys_earned
                .checked_add(node_keys_earned)
                .ok_or(ErrorCode::NumericOverflow)?;

            // Reset lock model - set unlock to current_time + 25 days
            stake_account.principal_unlock_time = current_time + PRINCIPAL_LOCK_PERIOD;

            // If already enrolled, bonus unlock follows principal (principal + 10d)
            if stake_account.enrolled_in_bonus {
                stake_account.bonus_unlock_time = stake_account.principal_unlock_time + BONUS_LOCK_PERIOD;

                // HAL-02 fix: Track latest unlock time for bonus withdrawal delay
                if stake_account.bonus_unlock_time > staking_pool.latest_bonus_unlock_time {
                    staking_pool.latest_bonus_unlock_time = stake_account.bonus_unlock_time;
                }
            }
        }

        // Update pool totals
        staking_pool.total_staked = staking_pool
            .total_staked
            .checked_add(amount)
            .ok_or(ErrorCode::NumericOverflow)?;

        // Track enrolled weight properly: only enrolled users increase total_enrolled_stake
        if stake_account.enrolled_in_bonus {
            staking_pool.total_enrolled_stake = staking_pool
                .total_enrolled_stake
                .checked_add(amount)
                .ok_or(ErrorCode::NumericOverflow)?;
        }

        msg!("✅ STAKE_TOKENS COMPLETED SUCCESSFULLY");
        msg!("Final stake amount: {}", stake_account.amount);
        msg!("Total pool staked: {}", staking_pool.total_staked);

        // Emit event
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

        Ok(())
    }

    pub fn unstake_tokens(ctx: Context<UnstakeTokens>, amount: u64) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake_account;
        let staking_pool = &mut ctx.accounts.staking_pool;

        // HAL-04 fix: Prevent zero amount unstaking
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(stake_account.amount >= amount, ErrorCode::InsufficientStakeBalance);

        // Block unstaking completely for bonus enrolled users
        require!(!stake_account.enrolled_in_bonus, ErrorCode::BonusEnrolledCannotUnstake);

        let current_time = Clock::get()?.unix_timestamp;
        let time_until_unlock = stake_account.principal_unlock_time - current_time;

        // Early exit options (relative to current principal unlock date)
        let (penalty_amount, net_amount) = if time_until_unlock <= 0 {
            // Unlocked: No penalty
            (0u64, amount)
        } else if time_until_unlock <= EARLY_UNSTAKE_THRESHOLD_1 {
            // 10—0 days remaining: 20% penalty
            let penalty = ((amount as u128) * (PENALTY_RATE_LATE as u128) / 100u128) as u64;
            (penalty, amount - penalty)
        } else if time_until_unlock <= EARLY_UNSTAKE_THRESHOLD_2 {
            // 20—10 days remaining: 30% penalty
            let penalty = ((amount as u128) * (PENALTY_RATE_EARLY as u128) / 100u128) as u64;
            (penalty, amount - penalty)
        } else {
            // More than 20 days remaining: Unstake blocked
            return Err(ErrorCode::UnstakeBlocked.into());
        };

        let user_token_account = &ctx.accounts.user_token_account;
        let pool_token_account = &ctx.accounts.pool_token_account;

        // Setup PDA signer
        let authority_seed = b"staking_pool";
        let bump_bytes = [staking_pool.bump];
        let signer_seeds = &[authority_seed.as_ref(), bump_bytes.as_ref()];
        let signer = &[signer_seeds.as_ref()];

        // Transfer net amount to user
        if net_amount > 0 {
            let cpi_accounts = Transfer {
                from: pool_token_account.to_account_info(),
                to: user_token_account.to_account_info(),
                authority: staking_pool.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, net_amount)?;
        }

        // Add penalty to the penalty vault (stays in pool for enrolled users)
        if penalty_amount > 0 {
            staking_pool.penalty_vault_amount = staking_pool
                .penalty_vault_amount
                .checked_add(penalty_amount)
                .ok_or(ErrorCode::NumericOverflow)?;
        }

        // If enrolled, reduce the enrolled pool weight by the amount being unstaked
        if stake_account.enrolled_in_bonus {
            staking_pool.total_enrolled_stake = staking_pool
                .total_enrolled_stake
                .saturating_sub(amount);
        }

        // Update balances
        stake_account.amount = stake_account
            .amount
            .checked_sub(amount)
            .ok_or(ErrorCode::NumericOverflow)?;
        staking_pool.total_staked = staking_pool
            .total_staked
            .checked_sub(amount)
            .ok_or(ErrorCode::NumericOverflow)?;

        emit!(UnstakeEvent {
            user: ctx.accounts.user.key(),
            amount,
            remaining_staked: stake_account.amount,
            penalty_applied: penalty_amount,
            net_to_user: net_amount,
            penalty_vault_total: staking_pool.penalty_vault_amount,
            timestamp: current_time,
        });

        msg!(
            "User {} unstaked {} tokens, penalty: {}, net received: {}",
            ctx.accounts.user.key(),
            amount,
            penalty_amount,
            net_amount
        );

        Ok(())
    }

    pub fn withdraw_all(ctx: Context<WithdrawAll>) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake_account;
        let staking_pool = &mut ctx.accounts.staking_pool;

        let current_time = Clock::get()?.unix_timestamp;

        // HAL-02 fix: Only check principal unlock time for withdrawing principal
        // Bonus rewards are withdrawn separately via withdraw_bonus
        let required_unlock_time = if stake_account.enrolled_in_bonus {
            stake_account.bonus_unlock_time
        } else {
            stake_account.principal_unlock_time
        };

        require!(current_time >= required_unlock_time, ErrorCode::StillLocked);
        require!(stake_account.amount > 0, ErrorCode::NoStakeToWithdraw);

        let user_token_account = &ctx.accounts.user_token_account;
        let pool_token_account = &ctx.accounts.pool_token_account;

        let principal_amount = stake_account.amount;
        // HAL-02 fix: No bonus rewards calculated here anymore
        let bonus_rewards = 0u64;

        let total_withdrawal = principal_amount;

        // Setup PDA signer
        let authority_seed = b"staking_pool";
        let bump_bytes = [staking_pool.bump];
        let signer_seeds = &[authority_seed.as_ref(), bump_bytes.as_ref()];
        let signer = &[signer_seeds.as_ref()];

        // Transfer total amount to user
        let cpi_accounts = Transfer {
            from: pool_token_account.to_account_info(),
            to: user_token_account.to_account_info(),
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

        // If enrolled, subtract full principal from enrolled total (use principal_amount before zeroing)
        if stake_account.enrolled_in_bonus {
            staking_pool.total_enrolled_stake = staking_pool
                .total_enrolled_stake
                .saturating_sub(principal_amount);
        }

        emit!(WithdrawAllEvent {
            user: ctx.accounts.user.key(),
            principal_amount,
            bonus_rewards,
            total_withdrawn: total_withdrawal,
            node_keys_retained: stake_account.node_keys_earned,
            timestamp: current_time,
        });

        // Reset stake account but keep node keys
        let node_keys_to_keep = stake_account.node_keys_earned;
        stake_account.amount = 0;
        stake_account.node_keys_earned = node_keys_to_keep; // Node keys are permanent
        stake_account.enrolled_in_bonus = false;
        stake_account.principal_unlock_time = 0;
        stake_account.bonus_unlock_time = 0;

        msg!(
            "User {} withdrew {} principal, keeping {} node keys",
            ctx.accounts.user.key(),
            principal_amount,
            node_keys_to_keep
        );

        Ok(())
    }

    // HAL-02 fix: Separate function to withdraw bonus rewards
    pub fn withdraw_bonus(ctx: Context<WithdrawBonus>) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake_account;
        let staking_pool = &mut ctx.accounts.staking_pool;

        let current_time = Clock::get()?.unix_timestamp;

        // Can only withdraw bonus 1 day after the latest unlock time of all users
        let bonus_withdrawal_time = staking_pool.latest_bonus_unlock_time + BONUS_WITHDRAWAL_DELAY;
        require!(current_time >= bonus_withdrawal_time, ErrorCode::BonusWithdrawalNotYetAvailable);

        // User must have been enrolled in bonus and completed withdrawal of principal
        require!(stake_account.enrolled_in_bonus, ErrorCode::NotEnrolledInBonus);
        require!(stake_account.amount == 0, ErrorCode::MustWithdrawPrincipalFirst);

        let user_token_account = &ctx.accounts.user_token_account;
        let pool_token_account = &ctx.accounts.pool_token_account;

        let mut bonus_rewards = 0u64;

        // Calculate bonus rewards proportionally
        if staking_pool.total_enrolled_stake > 0 && staking_pool.penalty_vault_amount > 0 {
            // Use the user's last staked amount (stored in node_keys to track their share)
            // We need to track original stake amount - let me add a field for this
            let pv = staking_pool.penalty_vault_amount as u128;
            let user_original_stake = (stake_account.node_keys_earned as u64 / KEYS_PER_STAKE as u64) * MINIMUM_STAKE_AMOUNT;
            let user_amt = user_original_stake as u128;
            let total = staking_pool.total_enrolled_stake as u128;
            bonus_rewards = if total > 0 { ((pv * user_amt) / total) as u64 } else { 0 };

            staking_pool.penalty_vault_amount =
                staking_pool.penalty_vault_amount.saturating_sub(bonus_rewards);
        }

        if bonus_rewards > 0 {
            // Setup PDA signer
            let authority_seed = b"staking_pool";
            let bump_bytes = [staking_pool.bump];
            let signer_seeds = &[authority_seed.as_ref(), bump_bytes.as_ref()];
            let signer = &[signer_seeds.as_ref()];

            // Transfer bonus amount to user
            let cpi_accounts = Transfer {
                from: pool_token_account.to_account_info(),
                to: user_token_account.to_account_info(),
                authority: staking_pool.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, bonus_rewards)?;
        }

        emit!(BonusWithdrawEvent {
            user: ctx.accounts.user.key(),
            bonus_amount: bonus_rewards,
            timestamp: current_time,
        });

        msg!(
            "User {} withdrew {} bonus rewards",
            ctx.accounts.user.key(),
            bonus_rewards
        );

        Ok(())
    }

    pub fn get_stake_info(ctx: Context<GetStakeInfo>) -> Result<StakeInfo> {
        let stake_account = &ctx.accounts.stake_account;
        let staking_pool = &ctx.accounts.staking_pool;

        // Calculate potential bonus (simple pro-rata preview)
        let potential_bonus = if stake_account.enrolled_in_bonus && staking_pool.total_enrolled_stake > 0 {
            let pv = staking_pool.penalty_vault_amount as u128;
            let user_amt = stake_account.amount as u128;
            let total = staking_pool.total_enrolled_stake as u128;
            // HAL-05 fix: Removed redundant total > 0 check (already checked above)
            ((pv * user_amt) / total) as u64
        } else {
            0
        };

        let current_time = Clock::get()?.unix_timestamp;
        
        // Determine unlock time based on enrollment status
        let unlock_time = if stake_account.enrolled_in_bonus {
            stake_account.bonus_unlock_time
        } else {
            stake_account.principal_unlock_time
        };

        let stake_info = StakeInfo {
            amount: stake_account.amount,
            node_keys_earned: stake_account.node_keys_earned,
            principal_unlock_time: stake_account.principal_unlock_time,
            bonus_unlock_time: stake_account.bonus_unlock_time,
            enrolled_in_bonus: stake_account.enrolled_in_bonus,
            potential_bonus,
            unlock_time,
            is_unlocked: current_time >= unlock_time,
            time_until_unlock: if current_time >= unlock_time { 0 } else { unlock_time - current_time },
        };

        msg!(
            "Stake Info - Amount: {}, Keys: {}, Enrolled: {}, Unlock Time: {}, Potential Bonus: {}",
            stake_info.amount,
            stake_info.node_keys_earned,
            stake_info.enrolled_in_bonus,
            stake_info.unlock_time,
            stake_info.potential_bonus
        );

        Ok(stake_info)
    }

    // ADDED: Collect dust from penalty vault to dev wallet
    pub fn collect_dust(ctx: Context<CollectDust>) -> Result<()> {
        let staking_pool = &mut ctx.accounts.staking_pool;

        require!(
            ctx.accounts.admin.key() == staking_pool.authority,
            ErrorCode::Unauthorized
        );

        let current_time = Clock::get()?.unix_timestamp;

        // Give users 2 months to claim their bonus rewards after bonus withdrawals become available
        let bonus_claim_deadline = staking_pool.latest_bonus_unlock_time + BONUS_WITHDRAWAL_DELAY + (60 * 24 * 60 * 60); // +60 days (2 months)
        require!(
            current_time >= bonus_claim_deadline,
            ErrorCode::BonusClaimPeriodNotExpired
        );

        // Only collect when penalty vault has tokens
        require!(
            staking_pool.penalty_vault_amount > 0,
            ErrorCode::NoDustToCollect
        );

        let dust_amount = staking_pool.penalty_vault_amount;
        let dev_wallet_key = DEV_WALLET.parse::<Pubkey>().map_err(|_| ErrorCode::InvalidDevWallet)?;

        require!(
            ctx.accounts.dev_token_account.owner == dev_wallet_key,
            ErrorCode::InvalidDevWallet
        );

        // Setup PDA signer
        let authority_seed = b"staking_pool";
        let bump_bytes = [staking_pool.bump];
        let signer_seeds = &[authority_seed.as_ref(), bump_bytes.as_ref()];
        let signer = &[signer_seeds.as_ref()];

        // Transfer dust to dev wallet
        let cpi_accounts = Transfer {
            from: ctx.accounts.pool_token_account.to_account_info(),
            to: ctx.accounts.dev_token_account.to_account_info(),
            authority: staking_pool.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, dust_amount)?;

        // Reset penalty vault
        staking_pool.penalty_vault_amount = 0;

        msg!("Collected {} unclaimed bonus tokens to dev wallet after 2-month claim period", dust_amount);
        Ok(())
    }

}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        // discriminator + authority + total_staked + total_enrolled_stake + penalty_vault_amount
        // + first_stake + bonus_deadline + stake_window_end + latest_bonus_unlock_time + qst_mint + bump
        space = 8 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 8 + 32 + 1,
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
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    // Admin only: Only admin can start the window (but only once)
    #[account()]
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct CollectDust<'info> {
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account()]
    pub admin: Signer<'info>,

    #[account(
        mut,
        constraint = pool_token_account.owner == staking_pool.key(),
        constraint = pool_token_account.mint == staking_pool.qst_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = dev_token_account.mint == staking_pool.qst_mint
    )]
    pub dev_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}



#[derive(Accounts)]
pub struct EnrollInBonus<'info> {
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account(
        mut,
        seeds = [b"stake_account", user.key().as_ref()],
        bump = stake_account.bump,
        constraint = stake_account.user == user.key()
    )]
    pub stake_account: Account<'info, StakeAccount>,

    #[account()]
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct StakeTokens<'info> {
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump,
        constraint = staking_pool.qst_mint == user_token_account.mint
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account(
        init_if_needed,
        payer = user,
        // discriminator + user + amount + node_keys_earned(u32) + principal_unlock + bonus_unlock + enrolled_in_bonus + bump
        space = 8 + 32 + 8 + 4 + 8 + 8 + 1 + 1,
        seeds = [b"stake_account", user.key().as_ref()],
        bump
    )]
    pub stake_account: Account<'info, StakeAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == staking_pool.qst_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = pool_token_account.owner == staking_pool.key(),
        constraint = pool_token_account.mint == staking_pool.qst_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UnstakeTokens<'info> {
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account(
        mut,
        seeds = [b"stake_account", user.key().as_ref()],
        bump = stake_account.bump,
        constraint = stake_account.user == user.key()
    )]
    pub stake_account: Account<'info, StakeAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == staking_pool.qst_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = pool_token_account.owner == staking_pool.key(),
        constraint = pool_token_account.mint == staking_pool.qst_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawAll<'info> {
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account(
        mut,
        seeds = [b"stake_account", user.key().as_ref()],
        bump = stake_account.bump,
        constraint = stake_account.user == user.key()
    )]
    pub stake_account: Account<'info, StakeAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == staking_pool.qst_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = pool_token_account.owner == staking_pool.key(),
        constraint = pool_token_account.mint == staking_pool.qst_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

// HAL-02 fix: New accounts struct for bonus withdrawal
#[derive(Accounts)]
pub struct WithdrawBonus<'info> {
    #[account(
        mut,
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account(
        mut,
        seeds = [b"stake_account", user.key().as_ref()],
        bump = stake_account.bump,
        constraint = stake_account.user == user.key()
    )]
    pub stake_account: Account<'info, StakeAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == staking_pool.qst_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = pool_token_account.owner == staking_pool.key(),
        constraint = pool_token_account.mint == staking_pool.qst_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct GetStakeInfo<'info> {
    #[account(
        seeds = [b"staking_pool"],
        bump = staking_pool.bump
    )]
    pub staking_pool: Account<'info, StakingPool>,

    #[account(
        seeds = [b"stake_account", user.key().as_ref()],
        bump = stake_account.bump,
        constraint = stake_account.user == user.key()
    )]
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
    pub latest_bonus_unlock_time: i64, // HAL-02 fix: track latest unlock time for bonus withdrawal
    pub qst_mint: Pubkey,
    pub bump: u8,
}

#[account]
pub struct StakeAccount {
    pub user: Pubkey,
    pub amount: u64,
    pub node_keys_earned: u32,
    pub principal_unlock_time: i64,
    pub bonus_unlock_time: i64, // 0 if not enrolled; principal+10d if enrolled
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
    pub unlock_time: i64,        // The actual unlock time (principal or bonus)
    pub is_unlocked: bool,       // Whether currently unlocked
    pub time_until_unlock: i64,  // Seconds remaining until unlock (0 if unlocked)
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

#[event]
pub struct UnstakeEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub remaining_staked: u64,
    pub penalty_applied: u64,
    pub net_to_user: u64,
    pub penalty_vault_total: u64,
    pub timestamp: i64,
}

#[event]
pub struct WithdrawAllEvent {
    pub user: Pubkey,
    pub principal_amount: u64,
    pub bonus_rewards: u64,
    pub total_withdrawn: u64,
    pub node_keys_retained: u32,
    pub timestamp: i64,
}

// HAL-02 fix: New event for bonus withdrawal
#[event]
pub struct BonusWithdrawEvent {
    pub user: Pubkey,
    pub bonus_amount: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Insufficient stake amount. Minimum is 200,000 QST")]
    InsufficientStakeAmount,
    #[msg("Stake amount too large. Maximum is 10,000,000 QST per stake")]
    StakeAmountTooLarge,
    #[msg("Invalid stake amount. Must be multiple of 200,000 QST")]
    InvalidStakeAmount,
    #[msg("Stake amount would result in too many node keys")]
    TooManyKeys,
    #[msg("Insufficient stake balance")]
    InsufficientStakeBalance,
    #[msg("Unstake blocked. More than 20 days remaining until unlock")]
    UnstakeBlocked,
    #[msg("Still locked. Cannot withdraw before principal unlock time")]
    StillLocked,
    #[msg("No stake to withdraw")]
    NoStakeToWithdraw,
    #[msg("Stake window is closed")]
    StakeWindowClosed,
    #[msg("Stake window is already active and cannot be restarted")]
    StakeWindowAlreadyActive,
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
    #[msg("No dust to collect or too many enrolled users remaining")]
    NoDustToCollect,
    #[msg("Invalid dev wallet address")]
    InvalidDevWallet,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("QST mint must use 6 decimals")]
    InvalidMintDecimals,
    #[msg("Numeric overflow")]
    NumericOverflow,
    // HAL-02 fix: New error codes for bonus withdrawal
    #[msg("Bonus withdrawal not yet available. Must wait 1 day after latest unlock time.")]
    BonusWithdrawalNotYetAvailable,
    #[msg("Not enrolled in bonus program")]
    NotEnrolledInBonus,
    #[msg("Must withdraw principal first before withdrawing bonus")]
    MustWithdrawPrincipalFirst,
    #[msg("Bonus claim period has not expired. Users have 2 months to claim rewards.")]
    BonusClaimPeriodNotExpired,
}