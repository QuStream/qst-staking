# QST Staking Protocol

A secure time-locked staking protocol for QST tokens on Solana, enabling users to earn node keys and bonus rewards through commitment-based staking.

## 🚀 Quick Start for Reviewers

### Code Review Only
```bash
git clone https://github.com/your-username/qst-staking.git
cd qst-staking
anchor build    # Builds mainnet version
```

### Full Testing Experience
```bash
git clone https://github.com/your-username/qst-staking.git
cd qst-staking
chmod +x scripts/setup-environment.sh
npm run test:local    # Runs complete test suite with devnet version
```

## Overview

The QST Staking Protocol implements a sophisticated staking mechanism where users can:
- **Stake QST tokens** for 30-day lock periods
- **Earn node keys** (2 keys per 200,000 QST minimum)
- **Enroll in bonus program** for additional 10-day commitment
- **Share penalty rewards** from early unstakers (bonus enrollees only)

## 📋 Dual Environment Setup

This repository includes **both mainnet and devnet/testing versions** for comprehensive review:

| Version | Purpose | Min Stake | Lock Time | Decimals |
|---------|---------|-----------|-----------|----------|
| **Mainnet** | Production deployment | 200,000 QST | 30+10 days | 6 |
| **Devnet** | Community testing | 0.002 QST | 1+0.5 minutes | 9 |

## Key Features

### 🔒 **Time-Lock Staking**
- **Principal Lock**: 30 days from last stake (1 minute in devnet)
- **Bonus Lock**: Additional 10 days for bonus enrollees (30 seconds in devnet)
- **Reset Model**: Each new stake resets unlock time

### 🎯 **Node Key System**
- Earn 2 node keys per 200,000 QST staked (0.002 QST in devnet)
- Keys are **permanent** - retained even after unstaking
- Used for network participation rights

### 💰 **Bonus Reward Program**
- **48-hour enrollment window** after stake window opens (2 minutes in devnet)
- Bonus enrollees share penalty fees from early unstakers
- **Full commitment**: No early unstaking allowed once enrolled
- Pro-rata distribution based on stake size

### ⚡ **Stake Windows**
- **14-day staking windows** set by admin (5 minutes in devnet)
- **48-hour bonus enrollment period** within each window (2 minutes in devnet)
- Mainnet: Admin controlled | Devnet: Auto-restart for testing

## Testing Instructions

### For Community Reviewers

#### Option 1: Code Review + Build Verification
```bash
# Clone and review the code
git clone <repo-url>
cd qst-staking

# Build mainnet version (no deployment needed)
anchor build
```

#### Option 2: Full Local Testing (Recommended)
```bash
# Switch to testing environment and run full test suite
npm run test:local
```

This runs a **complete 2-minute test scenario**:
- Sets up local Solana validator
- Creates test QST mint with small amounts
- Tests all functions: stake, enroll, unstake, withdraw
- Includes time-based testing with short lock periods
- Verifies bonus reward distribution

#### Option 3: Individual Test Scenarios
```bash
# Test mainnet build
npm run build:mainnet

# Test devnet build  
npm run build:devnet

# Run devnet tests on actual devnet
npm run test:devnet
```

### Available Scripts

```bash
# Environment Setup
npm run setup:mainnet      # Configure for mainnet deployment
npm run setup:devnet       # Configure for local/devnet testing

# Building
npm run build              # Build current environment
npm run build:mainnet      # Build mainnet version
npm run build:devnet       # Build devnet version

# Testing
npm run test:local         # Full local test with validator
npm run test:devnet        # Test on devnet network
npm run test              # Test without validator

# Deployment
npm run deploy:mainnet     # Deploy to mainnet
npm run deploy:devnet      # Deploy to devnet
```

## Economics

### Mainnet (Production)
- **Minimum Stake**: 200,000 QST (6 decimals)
- **Maximum Stake**: 10,000,000 QST per transaction
- **Lock Periods**: 30 days principal + 10 days bonus
- **Early Unstake Penalties**: 30% (>20 days) | 20% (10-20 days) | 0% (<10 days)

### Devnet (Testing)
- **Minimum Stake**: 0.002 QST (9 decimals)
- **Maximum Stake**: 10 QST per transaction  
- **Lock Periods**: 1 minute principal + 30 seconds bonus
- **Early Unstake Penalties**: 30% (>40s) | 20% (20-40s) | 0% (<20s)

## Security Features

### 🛡️ **Access Controls**
- Initialization protected against hijacking
- Admin-only functions with proper authorization
- Emergency pause capability (mainnet only)

### 🔐 **Overflow Protection**
- Safe arithmetic with u128 intermediates
- Checked conversions and bounds validation
- Protection against type truncation

### ⚖️ **Commitment Enforcement**
- Bonus enrollees cannot unstake early
- Proper lock time validation (30 vs 40 days)
- State tracking prevents manipulation

### 🧹 **Dust Management**
- Precision loss collected by dev wallet
- Prevents permanent token locking
- Transparent dust collection process

## Contract Architecture

```
StakingPool (PDA)
├── authority: Admin wallet
├── total_staked: Total QST in pool
├── total_enrolled_stake: QST from bonus enrollees
├── penalty_vault_amount: Accumulated penalties
└── timing: Window and deadline tracking

StakeAccount (per user)
├── amount: User's staked QST
├── node_keys_earned: Permanent node keys
├── principal_unlock_time: 30-day unlock
├── bonus_unlock_time: 40-day unlock (if enrolled)
└── enrolled_in_bonus: Bonus program status
```

## Functions

| Function | Description | Access | Devnet Changes |
|----------|-------------|---------|----------------|
| `initialize` | Set up staking pool | Dev wallet only | Same |
| `start_stake_window` | Begin stake window | Admin only | Anyone (testing) |
| `stake_tokens` | Stake QST tokens | Public (during window) | Auto-starts window |
| `enroll_in_bonus` | Join bonus program | Public (48h window) | 2min window |
| `unstake_tokens` | Early unstake with penalty | Public (non-enrolled) | Faster thresholds |
| `withdraw_all` | Withdraw after unlock | Public (after unlock) | 90s total wait |
| `get_stake_info` | Query user stake data | Public | Returns test data |

## Deployment Guide

### Prerequisites
- Anchor CLI 0.30.1+
- Solana CLI 1.18+
- Dev wallet with SOL for deployment
- QST token mint

### Mainnet Deployment
```bash
# 1. Configure for mainnet
npm run setup:mainnet

# 2. Build and deploy
anchor build
anchor deploy --provider.cluster mainnet

# 3. Create pool token account  
spl-token create-account <QST_MINT> --owner <STAKING_POOL_PDA>

# 4. Initialize staking pool
anchor invoke initialize <ADMIN_WALLET> \
  --accounts stakingPool:<PDA> qstMint:<MINT> payer:<DEV_WALLET>

# 5. Update program ID in code and redeploy if needed
```

### Devnet Testing
```bash
# Quick setup and test
npm run test:local
```

## Security Considerations

### ⚠️ **Before Using**
- **Security Audit Recommended**: This contract handles financial assets
- **Test Thoroughly**: Use devnet testing extensively
- **Admin Key Security**: Protect admin private keys properly
- **Emergency Procedures**: Understand pause and recovery (mainnet)

### 🔍 **Code Review Focus Areas**
- **Initialization security**: Dev wallet deployment protection
- **Arithmetic safety**: u128 intermediate calculations
- **Lock time logic**: 25-day vs 35-day enforcement
- **Bonus commitment**: Enrolled users cannot unstake early
- **Access controls**: Admin-only functions properly protected

## File Structure

```
qst-staking/
├── programs/qst-staking/src/
│   ├── lib.rs                  # Mainnet version (active)
│   └── lib_devnet.rs           # Devnet version for testing
├── tests/
│   ├── qst-staking.ts          # Mainnet tests
│   └── qst-staking-devnet.ts   # Devnet tests (runnable)
├── scripts/
│   └── setup-environment.sh   # Environment switcher
├── Anchor.toml                 # Mainnet config (active)
├── Anchor_devnet.toml         # Devnet config
└── package.json               # NPM scripts for both environments
```

## Developer Information

- **Dev Wallet**: `oejJbosh9dQKKVNNPEkDZxkiTNMkMjAKjYftMGQA2ww`
- **Deployment Control**: Only dev wallet can deploy and initialize
- **Dust Collection**: Precision loss from calculations goes to dev wallet
- **License**: MIT
- **Audit Status**: Pending professional security audit

## Contributing

Community review and feedback welcomed. Please:

1. **Review the code** - Focus on security-critical functions
2. **Test locally** - Use `npm run test:local` for hands-on verification
3. **Report issues** - Use GitHub issues for any findings
4. **Suggest improvements** - Security and efficiency improvements welcome

## Testing Workflow for Reviewers

```bash
# 1. Clone and setup
git clone <repo-url> && cd qst-staking
npm install

# 2. Review mainnet code (production version)
cat programs/qst-staking/src/lib.rs

# 3. Run comprehensive tests (switches to devnet version automatically)
npm run test:local

# 4. Examine test results and behavior
# Tests create real token accounts, stake, enroll in bonus, 
# wait for lock periods, and verify all functions work correctly
```

## Links

- **QST Token**: [Add QST token mint address]
- **Documentation**: [Add docs link if available]  
- **Community**: [Add community links]

---

**⚠️ Disclaimer**: This smart contract is provided as-is. Use at your own risk. Ensure you understand the locking mechanisms before staking tokens. The devnet version is for testing only and uses different parameters than mainnet.