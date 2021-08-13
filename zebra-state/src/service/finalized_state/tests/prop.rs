use std::env;

use zebra_chain::{block::Height, parameters::NetworkUpgrade};
use zebra_test::prelude::*;

use crate::tests::FakeChainHelper;

use crate::{
    config::Config,
    service::{
        arbitrary::PreparedChain,
        finalized_state::{FinalizedBlock, FinalizedState},
    },
    ContextuallyValidBlock,
};

const DEFAULT_PARTIAL_CHAIN_PROPTEST_CASES: u32 = 1;

#[test]
fn blocks_with_v5_transactions() -> Result<()> {
    zebra_test::init();
    proptest!(ProptestConfig::with_cases(env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PARTIAL_CHAIN_PROPTEST_CASES)),
        |((chain, count, network, _history_tree) in PreparedChain::default())| {
            let mut state = FinalizedState::new(&Config::ephemeral(), network);
            let mut height = Height(0);
            // use `count` to minimize test failures, so they are easier to diagnose
            for block in chain.iter().take(count) {
                let hash = state.commit_finalized_direct(
                    FinalizedBlock::from(ContextuallyValidBlock::from(block.clone())),
                    "blocks_with_v5_transactions test"
                );
                prop_assert_eq!(Some(height), state.finalized_tip_height());
                prop_assert_eq!(hash.unwrap(), block.hash);
                // TODO: check that the nullifiers were correctly inserted (#2230)
                height = Height(height.0 + 1);
            }
    });

    Ok(())
}

#[test]
fn all_upgrades() -> Result<()> {
    zebra_test::init();
    proptest!(ProptestConfig::with_cases(1),
        |((chain, _count, network, _history_tree) in PreparedChain::default().no_shrink())| {
            let mut state = FinalizedState::new(&Config::ephemeral(), network);
            let mut height = Height(0);
            for block in chain.iter() {
                let block_hash = block.hash;
                let mut block = block.block.clone();
                let current_height = block.coinbase_height().unwrap();
                let network_upgrade = NetworkUpgrade::current(network, current_height);
                if current_height == NetworkUpgrade::Heartwood.activation_height(network).unwrap() {
                    block = block.set_block_commitment([0u8; 32]);
                }
                println!("{:?} {:?}", current_height, network_upgrade);
                let hash = state.commit_finalized_direct(
                    FinalizedBlock::from(block.clone()),
                    "blocks_with_v5_transactions test"
                ).unwrap();
                prop_assert_eq!(Some(height), state.finalized_tip_height());
                // prop_assert_eq!(hash, block_hash);
                // TODO: check that the nullifiers were correctly inserted (#2230)
                height = Height(height.0 + 1);
            }
    });

    Ok(())
}
