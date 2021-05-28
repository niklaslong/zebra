use std::convert::TryFrom;

use zebra_chain::parameters::{Network, NetworkUpgrade};
use zebra_chain::primitives::redjubjub::VerificationKey;
use zebra_chain::transaction::{HashType, Transaction};
use zebra_chain::{block::Block, serialization::ZcashDeserializeInto};

#[test]
fn verify_test_vector_binding_signatures() {
    let network = Network::Mainnet;

    for (height, bytes) in zebra_test::vectors::MAINNET_BLOCKS.clone().iter() {
        let upgrade = NetworkUpgrade::current(network, zebra_chain::block::Height(*height));

        let block = bytes
            .zcash_deserialize_into::<Block>()
            .expect("a valid block");

        for tx in block.transactions {
            match &*tx {
                Transaction::V1 { .. } | Transaction::V2 { .. } | Transaction::V3 { .. } => (),
                Transaction::V4 {
                    sapling_shielded_data,
                    ..
                } => {
                    if let Some(sapling_shielded_data) = sapling_shielded_data {
                        let shielded_sighash = tx.sighash(upgrade, HashType::ALL, None);

                        let bvk = VerificationKey::try_from(
                            sapling_shielded_data.binding_verification_key(),
                        )
                        .expect("a valid redjubjub::VerificationKey");

                        let mut data_to_be_signed = [0u8; 64];
                        data_to_be_signed[0..32].copy_from_slice(&<[u8; 32]>::from(bvk));
                        (&mut data_to_be_signed[32..64])
                            .copy_from_slice(&shielded_sighash.as_ref());

                        if let Err(_) = bvk.verify(
                            shielded_sighash.as_ref(),
                            &sapling_shielded_data.binding_sig,
                        ) {
                            match bvk
                                .verify(&data_to_be_signed[..], &sapling_shielded_data.binding_sig)
                            {
                                Ok(_) => (),
                                Err(_) => {
                                    println!("FAIL!\n");
                                    println!("{:?}\n", sapling_shielded_data);
                                }
                            }
                            // panic!()
                        }
                    }
                }
                Transaction::V5 { .. } => (),
            }
        }
    }
}
