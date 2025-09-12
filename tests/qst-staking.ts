import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { QstStakingMainnet } from "../target/types/qst_staking_mainnet";
import { 
  TOKEN_PROGRAM_ID, 
  createMint, 
  createAccount, 
  mintTo,
  getAccount
} from "@solana/spl-token";
import { expect } from "chai";

describe("QST Staking Protocol", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.QstStakingMainnet as Program<QstStakingMainnet>;
  
  // Test accounts
  let qstMint: anchor.web3.PublicKey;
  let adminWallet: anchor.web3.Keypair;
  let userWallet: anchor.web3.Keypair;
  let stakingPoolPda: anchor.web3.PublicKey;
  let userTokenAccount: anchor.web3.PublicKey;
  let poolTokenAccount: anchor.web3.PublicKey;
  let userStakeAccount: anchor.web3.PublicKey;
  
  // Test constants
  const MINIMUM_STAKE = new anchor.BN(200_000 * 1_000_000); // 200k QST
  const MAXIMUM_STAKE = new anchor.BN(10_000_000 * 1_000_000); // 10M QST

  before(async () => {
    // Generate test accounts
    adminWallet = anchor.web3.Keypair.generate();
    userWallet = anchor.web3.Keypair.generate();
    
    // Airdrop SOL for testing
    await provider.connection.requestAirdrop(adminWallet.publicKey, 5 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(userWallet.publicKey, 5 * anchor.web3.LAMPORTS_PER_SOL);
    
    // Create QST mint with 6 decimals
    qstMint = await createMint(
      provider.connection,
      adminWallet,
      adminWallet.publicKey,
      null,
      6
    );
    
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
    
    // Mint test tokens to user (1M QST)
    await mintTo(
      provider.connection,
      adminWallet,
      qstMint,
      userTokenAccount,
      adminWallet,
      1_000_000 * 1_000_000
    );
  });

  describe("Initialization", () => {
    it("Should initialize staking pool correctly", async () => {
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
    });
  });

  describe("Stake Window Management", () => {
    it("Should start stake window as admin", async () => {
      await program.methods
        .startStakeWindow()
        .accounts({
          stakingPool: stakingPoolPda,
          admin: adminWallet.publicKey,
        })
        .signers([adminWallet])
        .rpc();

      const poolAccount = await program.account.stakingPool.fetch(stakingPoolPda);
      expect(poolAccount.firstStakeTimestamp.toNumber()).to.be.greaterThan(0);
      expect(poolAccount.stakeWindowEnd.toNumber()).to.be.greaterThan(0);
    });
  });

  describe("Token Staking", () => {
    it("Should stake minimum amount successfully", async () => {
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
      expect(stakeAccount.amount.toString()).to.equal(MINIMUM_STAKE.toString());
      expect(stakeAccount.nodeKeysEarned).to.equal(2); // 200k QST = 2 keys
      expect(stakeAccount.enrolledInBonus).to.be.false;
    });

    it("Should reject stakes below minimum", async () => {
      const tooSmall = new anchor.BN(100_000 * 1_000_000); // 100k QST
      
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
      }
    });

    it("Should reject stakes above maximum", async () => {
      const tooLarge = new anchor.BN(20_000_000 * 1_000_000); // 20M QST
      
      try {
        await program.methods
          .stakeTokens(tooLarge)
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
        
        expect.fail("Should have rejected stake above maximum");
      } catch (error) {
        expect(error.toString()).to.include("StakeAmountTooLarge");
      }
    });
  });

  describe("Bonus Enrollment", () => {
    it("Should enroll in bonus program successfully", async () => {
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
      expect(stakeAccount.enrolledInBonus).to.be.true;
      expect(stakeAccount.bonusUnlockTime.toNumber()).to.be.greaterThan(
        stakeAccount.principalUnlockTime.toNumber()
      );
    });
  });

  describe("Stake Info Retrieval", () => {
    it("Should return comprehensive stake information", async () => {
      const stakeInfo = await program.methods
        .getStakeInfo()
        .accounts({
          stakingPool: stakingPoolPda,
          stakeAccount: userStakeAccount,
          user: userWallet.publicKey,
        })
        .signers([userWallet])
        .rpc();

      // This would return the StakeInfo struct if implemented
      // For now, just verify the transaction succeeds
      expect(stakeInfo).to.not.be.null;
    });
  });

  describe("Access Control", () => {
    it("Should reject non-admin attempting to start stake window", async () => {
      try {
        await program.methods
          .startStakeWindow()
          .accounts({
            stakingPool: stakingPoolPda,
            admin: userWallet.publicKey, // Non-admin trying to start window
          })
          .signers([userWallet])
          .rpc();
        
        expect.fail("Should have rejected non-admin");
      } catch (error) {
        expect(error.toString()).to.include("Unauthorized");
      }
    });
  });
});

// Helper functions for additional test scenarios
async function advanceTime(seconds: number) {
  // In a real test environment, you might use time manipulation
  // For now, this is a placeholder for time-based testing
  await new Promise(resolve => setTimeout(resolve, 1000));
}

async function getTokenBalance(tokenAccount: anchor.web3.PublicKey): Promise<number> {
  const account = await getAccount(provider.connection, tokenAccount);
  return Number(account.amount);
}