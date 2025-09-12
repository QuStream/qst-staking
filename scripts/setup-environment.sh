#!/bin/bash

# QST Staking Environment Setup Script
# Usage: ./scripts/setup-environment.sh [mainnet|devnet]

set -e

ENV=${1:-devnet}

echo "🚀 Setting up QST Staking for $ENV environment..."

case $ENV in
  "mainnet")
    echo "📋 Configuring for MAINNET deployment..."
    
    # Copy mainnet lib.rs as the active version
    cp programs/qst-staking/src/lib.rs programs/qst-staking/src/lib_mainnet_backup.rs 2>/dev/null || true
    echo "✅ Mainnet contract is already active in lib.rs"
    
    # Use mainnet Anchor.toml
    cp Anchor.toml Anchor_mainnet_backup.toml 2>/dev/null || true
    echo "✅ Using mainnet Anchor configuration"
    
    # Set Cargo.toml for mainnet
    sed -i.bak 's/name = "qst_staking_devnet"/name = "qst_staking_mainnet"/' programs/qst-staking/Cargo.toml
    
    echo ""
    echo "⚠️  MAINNET SETUP COMPLETE ⚠️"
    echo "- Review all code thoroughly before deployment"
    echo "- Ensure you have the correct admin keypair"
    echo "- Update program ID after deployment"
    echo "- Minimum stake: 200,000 QST"
    echo "- Lock periods: 30 days + 10 days bonus"
    echo ""
    ;;
    
  "devnet")
    echo "🧪 Configuring for DEVNET/LOCAL testing..."
    
    # Copy devnet version as active lib.rs
    if [ -f "programs/qst-staking/src/lib_devnet.rs" ]; then
      cp programs/qst-staking/src/lib.rs programs/qst-staking/src/lib_mainnet_backup.rs 2>/dev/null || true
      cp programs/qst-staking/src/lib_devnet.rs programs/qst-staking/src/lib.rs
      echo "✅ Switched to devnet contract version"
    else
      echo "❌ Devnet lib.rs not found! Using existing lib.rs"
    fi
    
    # Use devnet Anchor.toml
    if [ -f "Anchor_devnet.toml" ]; then
      cp Anchor.toml Anchor_mainnet_backup.toml 2>/dev/null || true
      cp Anchor_devnet.toml Anchor.toml
      echo "✅ Switched to devnet Anchor configuration"
    fi
    
    # Set Cargo.toml for devnet
    sed -i.bak 's/name = "qst_staking_mainnet"/name = "qst_staking_devnet"/' programs/qst-staking/Cargo.toml
    
    echo ""
    echo "🧪 DEVNET SETUP COMPLETE 🧪"
    echo "- Easy testing with small amounts"
    echo "- Minimum stake: 0.002 QST (9 decimals)"
    echo "- Lock periods: 1 minute + 30 seconds bonus"
    echo "- Auto-starting stake windows"
    echo ""
    ;;
    
  *)
    echo "❌ Invalid environment. Use 'mainnet' or 'devnet'"
    echo "Usage: ./scripts/setup-environment.sh [mainnet|devnet]"
    exit 1
    ;;
esac

echo "🔧 Installing dependencies..."
if command -v yarn &> /dev/null; then
  yarn install
elif command -v npm &> /dev/null; then
  npm install
else
  echo "⚠️  Please install Node.js and npm/yarn to install dependencies"
fi

echo ""
echo "🎯 Environment: $ENV"
echo "✅ Setup complete! You can now:"
echo "  - anchor build    # Build the program"
echo "  - anchor test     # Run tests"
echo "  - anchor deploy   # Deploy to target network"
echo ""

if [ "$ENV" = "devnet" ]; then
  echo "🧪 For devnet testing:"
  echo "  - Tests will run with small token amounts"
  echo "  - Lock periods are very short (1-2 minutes)"
  echo "  - Anyone can start stake windows"
  echo ""
fi