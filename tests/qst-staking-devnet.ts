import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { QstStakingDevnet } from "../target/types/qst_staking_devnet";
import { 
  TOKEN_PROGRAM_ID, 
  createMint, 
  createAccount, 
  mintTo,
  getAccount
} from "@solana/spl-token";
import { expect } from "chai";

describe("QST Staking Protocol - Devnet Testing", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.QstStakingDevnet as Program<QstStakingDevnet>;
  
  // Test accounts
  let qstMint: anchor.web3.PublicKey;
  let adminWallet: anchor.web3.Keypair;
  let userWallet: anchor.web3.Keypair;
  let stakingPoolPda: anchor.web3.PublicKey;
  let userTokenAccount: anchor.web3.PublicKey;
  let poolTokenAccount: anchor.web3.PublicKey;
  let userStakeAccount: anchor.web3.PublicKey;
  
  // DEVNET Test constants (much smaller for easy testing)
  const MINIMUM_STAKE = new anchor.BN(2_000_000); // 0.002 QST (9 decimals)
  const MAXIMUM_STAKE = new anchor.BN(10_000_000_000); // 10 QST (9 decimals)
  const TEST_STAKE_AMOUNT = new anchor.BN(4_000_000); // 0.004 QST (earns 4 node keys)

  before(async () => {
    console.log("Setting up devnet test environment...");
    
    // Generate test accounts
    adminWallet = anchor.web3.Keypair.generate();
    userWallet = anchor.web3.Keypair.generate();
    
    // Airdrop SOL for testing
    await provider.connection.requestAirdrop(adminWallet.publicKey, 5 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(userWallet.publicKey, 5 * anchor.web3.LAMPORTS_PER_SOL);
    
    // Create test QST mint with 9 decimals (easier for testing)
    qstMint = await createMint(
      provider.connection,
      adminWallet,
      adminWallet.publicKey,
      null,
      9 // 9 decimals for devnet testing
    );
    
    console.log("Created test QST mint:", qstMint.toString());
    
    // Find PDAs
    [stakingPoolPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("staking_pool")],
      program.programId
    );
    
    [userStakeAccount] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("stake_account"), userWallet.publicKey.toBuffer()],
      program.programId
    );
    
    // Create token accounts
    userTokenAccount = await createAccount(
      provider.connection,
      userWallet,
      qstMint,
      userWallet.publicKey
    );
    
    poolTokenAccount = await createAccount(
      provider.connection,
      adminWallet,
      qstMint,
      stakingPoolPda,
      undefined,
      TOKEN_PROGRAM_ID
    );
    
    // Mint test tokens to user (100 QST for plenty of testing)
    await mintTo(
      provider.connection,
      adminWallet,
      qstMint,
      userTokenAccount,
      adminWallet,
      100 * 1_000_000_000 // 100 QST with 9 decimals
    );
    
    console.log("Test setup complete!");
  });

  describe("Initialization", () => {
    it("Should initialize staking pool correctly", async () => {
      console.log("Testing initialization...");
      
      await program.methods
        .initialize(adminWallet.publicKey)
        .accounts({
          stakingPool: stakingPoolPda,
          qstMint: qstMint,
          payer: provider.wallet.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();

      const poolAccount = await program.account.stakingPool.fetch(stakingPoolPda);
      expect(poolAccount.authority.toString()).to.equal(adminWallet.publicKey.toString());
      expect(poolAccount.totalStaked.toNumber()).to.equal(0);
      expect(poolAccount.qstMint.toString()).to.equal(qstMint.toString());
      
      console.log("✅ Initialization successful");
    });
  });

  describe("Devnet Stake Window (Auto-start)", () => {
    it("Should auto-start stake window during first stake", async () => {
      console.log("Testing auto-starting stake window...");
      
      // No need to manually start window - devnet version auto-starts
      await program.methods
        .stakeTokens(TEST_STAKE_AMOUNT)
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: userStakeAccount,
          user: userWallet.publicKey,
          userTokenAccount: userTokenAccount,
          poolTokenAccount: poolTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([userWallet])
        .rpc();

      const poolAccount = await program.account.stakingPool.fetch(stakingPoolPda);
      expect(poolAccount.firstStakeTimestamp.toNumber()).to.be.greaterThan(0);
      expect(poolAccount.stakeWindowEnd.toNumber()).to.be.greaterThan(0);
      
      console.log("✅ Auto-start successful");
    });
  });

  describe("Token Staking", () => {
    it("Should stake minimum amount successfully", async () => {
      console.log("Testing minimum stake...");
      
      const initialBalance = await getTokenBalance(userTokenAccount);
      
      await program.methods
        .stakeTokens(MINIMUM_STAKE)
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: userStakeAccount,
          user: userWallet.publicKey,
          userTokenAccount: userTokenAccount,
          poolTokenAccount: poolTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([userWallet])
        .rpc();

      const stakeAccount = await program.account.stakeAccount.fetch(userStakeAccount);
      const finalBalance = await getTokenBalance(userTokenAccount);
      
      expect(stakeAccount.nodeKeysEarned).to.equal(4); // 0.004 total = 4 keys
      expect(stakeAccount.enrolledInBonus).to.be.false;
      expect(finalBalance).to.equal(initialBalance - Number(MINIMUM_STAKE));
      
      console.log("✅ Minimum stake successful, earned", stakeAccount.nodeKeysEarned, "keys");
    });

    it("Should reject stakes below minimum", async () => {
      console.log("Testing rejection of sub-minimum stake...");
      
      const tooSmall = new anchor.BN(1_000_000); // 0.001 QST
      
      try {
        await program.methods
          .stakeTokens(tooSmall)
          .accounts({
            stakingPool: stakingPoolPda,
            stakeAccount: userStakeAccount,
            user: userWallet.publicKey,
            userTokenAccount: userTokenAccount,
            poolTokenAccount: poolTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId,
          })
          .signers([userWallet])
          .rpc();
        
        expect.fail("Should have rejected stake below minimum");
      } catch (error) {
        expect(error.toString()).to.include("InsufficientStakeAmount");
        console.log("✅ Correctly rejected sub-minimum stake");
      }
    });
  });

  describe("Bonus Enrollment (Fast)", () => {
    it("Should enroll in bonus program successfully", async () => {
      console.log("Testing bonus enrollment...");
      
      await program.methods
        .enrollInBonus()
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: userStakeAccount,
          user: userWallet.publicKey,
        })
        .signers([userWallet])
        .rpc();

      const stakeAccount = await program.account.stakeAccount.fetch(userStakeAccount);
      const poolAccount = await program.account.stakingPool.fetch(stakingPoolPda);
      
      expect(stakeAccount.enrolledInBonus).to.be.true;
      expect(stakeAccount.bonusUnlockTime.toNumber()).to.be.greaterThan(
        stakeAccount.principalUnlockTime.toNumber()
      );
      expect(poolAccount.totalEnrolledStake.toString()).to.equal(stakeAccount.amount.toString());
      
      console.log("✅ Bonus enrollment successful");
    });

    it("Should prevent enrolled user from unstaking", async () => {
      console.log("Testing bonus commitment enforcement...");
      
      const unstakeAmount = new anchor.BN(1_000_000); // Try to unstake 0.001 QST
      
      try {
        await program.methods
          .unstakeTokens(unstakeAmount)
          .accounts({
            stakingPool: stakingPoolPda,
            stakeAccount: userStakeAccount,
            user: userWallet.publicKey,
            userTokenAccount: userTokenAccount,
            poolTokenAccount: poolTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .signers([userWallet])
          .rpc();
        
        expect.fail("Should have blocked unstaking for bonus-enrolled user");
      } catch (error) {
        expect(error.toString()).to.include("BonusEnrolledCannotUnstake");
        console.log("✅ Correctly blocked unstaking for enrolled user");
      }
    });
  });

  describe("Stake Info Retrieval", () => {
    it("Should return comprehensive stake information", async () => {
      console.log("Testing stake info retrieval...");
      
      const stakeInfo = await program.methods
        .getStakeInfo()
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: userStakeAccount,
          user: userWallet.publicKey,
        })
        .signers([userWallet])
        .view(); // Use view() to get return value

      expect(stakeInfo.amount.toNumber()).to.be.greaterThan(0);
      expect(stakeInfo.nodeKeysEarned).to.be.greaterThan(0);
      expect(stakeInfo.enrolledInBonus).to.be.true;
      expect(stakeInfo.isUnlocked).to.be.false; // Should still be locked
      expect(stakeInfo.timeUntilUnlock.toNumber()).to.be.greaterThan(0);
      
      console.log("✅ Stake info:", {
        amount: stakeInfo.amount.toString(),
        nodeKeys: stakeInfo.nodeKeysEarned,
        enrolled: stakeInfo.enrolledInBonus,
        timeUntilUnlock: stakeInfo.timeUntilUnlock.toString() + " seconds"
      });
    });
  });

  describe("Time-based Testing (Fast)", () => {
    it("Should allow withdrawal after lock period (fast testing)", async () => {
      console.log("Waiting for lock period to expire (90 seconds)...");
      
      // Wait for lock period (1 minute + 30 seconds bonus)
      await new Promise(resolve => setTimeout(resolve, 95000));
      
      const initialBalance = await getTokenBalance(userTokenAccount);
      
      await program.methods
        .withdrawAll()
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: userStakeAccount,
          user: userWallet.publicKey,
          userTokenAccount: userTokenAccount,
          poolTokenAccount: poolTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([userWallet])
        .rpc();

      const finalBalance = await getTokenBalance(userTokenAccount);
      const stakeAccount = await program.account.stakeAccount.fetch(userStakeAccount);
      
      expect(finalBalance).to.be.greaterThan(initialBalance);
      expect(stakeAccount.amount.toNumber()).to.equal(0);
      expect(stakeAccount.nodeKeysEarned).to.be.greaterThan(0); // Keys should be retained
      
      console.log("✅ Withdrawal successful after lock period");
      console.log("Final balance:", finalBalance);
      console.log("Retained node keys:", stakeAccount.nodeKeysEarned);
    }).timeout(120000); // 2 minute timeout for this test
  });

  describe("Multiple User Testing", () => {
    let user2Wallet: anchor.web3.Keypair;
    let user2TokenAccount: anchor.web3.PublicKey;
    let user2StakeAccount: anchor.web3.PublicKey;

    it("Should handle multiple users staking simultaneously", async () => {
      console.log("Setting up second user...");
      
      user2Wallet = anchor.web3.Keypair.generate();
      await provider.connection.requestAirdrop(user2Wallet.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
      
      [user2StakeAccount] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("stake_account"), user2Wallet.publicKey.toBuffer()],
        program.programId
      );
      
      user2TokenAccount = await createAccount(
        provider.connection,
        user2Wallet,
        qstMint,
        user2Wallet.publicKey
      );
      
      // Mint tokens to user2
      await mintTo(
        provider.connection,
        adminWallet,
        qstMint,
        user2TokenAccount,
        adminWallet,
        50 * 1_000_000_000 // 50 QST
      );
      
      // User2 stakes without bonus enrollment
      await program.methods
        .stakeTokens(TEST_STAKE_AMOUNT)
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: user2StakeAccount,
          user: user2Wallet.publicKey,
          userTokenAccount: user2TokenAccount,
          poolTokenAccount: poolTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([user2Wallet])
        .rpc();

      const poolAccount = await program.account.stakingPool.fetch(stakingPoolPda);
      const user2StakeAcc = await program.account.stakeAccount.fetch(user2StakeAccount);
      
      expect(poolAccount.totalStaked.toNumber()).to.be.greaterThan(TEST_STAKE_AMOUNT.toNumber());
      expect(user2StakeAcc.enrolledInBonus).to.be.false;
      
      console.log("✅ Multiple users handled correctly");
    });
  });
});

// Helper function to get token balance
async function getTokenBalance(tokenAccount: anchor.web3.PublicKey): Promise<number> {
  const account = await getAccount(anchor.AnchorProvider.env().connection, tokenAccount);
  return Number(account.amount);
}

// Export for use in other test files
export { getTokenBalance };