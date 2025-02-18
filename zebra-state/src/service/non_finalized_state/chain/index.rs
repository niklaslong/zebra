//! Transparent address indexes for non-finalized chains.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use mset::MultiSet;

use zebra_chain::{
    amount::{Amount, NegativeAllowed},
    transaction, transparent,
};

use crate::{OutputLocation, TransactionLocation, ValidateContextError};

use super::{RevertPosition, UpdateWith};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransparentTransfers {
    /// The partial chain balance for a transparent address.
    ///
    /// TODO:
    /// - to avoid [`ReadStateService`] response inconsistencies when a block has just been finalized,
    ///   revert UTXO receives and spends that are at a height less than or equal to the finalized tip.
    balance: Amount<NegativeAllowed>,

    /// The partial list of transactions that spent or received UTXOs to a transparent address.
    ///
    /// Since transactions can only be added to this set, it does not need special handling
    /// for [`ReadStateService`] response inconsistencies.
    ///
    /// The `getaddresstxids` RPC needs these transaction IDs to be sorted in chain order.
    tx_ids: MultiSet<transaction::Hash>,

    /// The partial list of UTXOs received by a transparent address.
    ///
    /// The `getaddressutxos` RPC doesn't need these transaction IDs to be sorted in chain order,
    /// but it might in future. So Zebra does it anyway.
    ///
    /// TODO:
    /// - to avoid [`ReadStateService`] response inconsistencies when a block has just been finalized,
    ///   combine the created UTXOs, combine the spent UTXOs, and then remove spent from created
    ///
    /// Optional:
    /// - store `Utxo`s in the chain, and just store the created locations for this address
    /// - if we add an OutputLocation to UTXO, remove this OutputLocation,
    ///   and use the inner OutputLocation to sort Utxos in chain order
    created_utxos: BTreeMap<OutputLocation, transparent::Output>,

    /// The partial list of UTXOs spent by a transparent address.
    ///
    /// The `getaddressutxos` RPC doesn't need these transaction IDs to be sorted in chain order,
    /// but it might in future. So Zebra does it anyway.
    ///
    /// Optional TODO:
    /// - store spent `Utxo`s by location in the chain, use the chain spent UTXOs to filter,
    ///   and stop storing spent UTXOs by address
    spent_utxos: BTreeSet<OutputLocation>,
}

// A created UTXO
//
// TODO: replace arguments with a struct
impl
    UpdateWith<(
        // The location of the UTXO
        &transparent::OutPoint,
        // The UTXO data
        // Includes the location of the transaction that created the output
        &transparent::OrderedUtxo,
    )> for TransparentTransfers
{
    fn update_chain_tip_with(
        &mut self,
        &(outpoint, created_utxo): &(&transparent::OutPoint, &transparent::OrderedUtxo),
    ) -> Result<(), ValidateContextError> {
        self.balance =
            (self.balance + created_utxo.utxo.output.value().constrain().unwrap()).unwrap();

        let transaction_location = transaction_location(created_utxo);
        let output_location = OutputLocation::from_outpoint(transaction_location, outpoint);

        let previous_entry = self
            .created_utxos
            .insert(output_location, created_utxo.utxo.output.clone());
        assert_eq!(
            previous_entry, None,
            "unexpected created output: duplicate update or duplicate UTXO",
        );

        self.tx_ids.insert(outpoint.hash);

        Ok(())
    }

    fn revert_chain_with(
        &mut self,
        &(outpoint, created_utxo): &(&transparent::OutPoint, &transparent::OrderedUtxo),
        _position: RevertPosition,
    ) {
        self.balance =
            (self.balance - created_utxo.utxo.output.value().constrain().unwrap()).unwrap();

        let transaction_location = transaction_location(created_utxo);
        let output_location = OutputLocation::from_outpoint(transaction_location, outpoint);

        let removed_entry = self.created_utxos.remove(&output_location);
        assert!(
            removed_entry.is_some(),
            "unexpected revert of created output: duplicate update or duplicate UTXO",
        );

        let tx_id_was_removed = self.tx_ids.remove(&outpoint.hash);
        assert!(
            tx_id_was_removed,
            "unexpected revert of created output transaction: \
             duplicate revert, or revert of an output that was never updated",
        );
    }
}

// A transparent input
//
// TODO: replace arguments with a struct
impl
    UpdateWith<(
        // The transparent input data
        &transparent::Input,
        // The hash of the transaction the input is from
        &transaction::Hash,
        // The output spent by the input
        // Includes the location of the transaction that created the output
        &transparent::OrderedUtxo,
    )> for TransparentTransfers
{
    fn update_chain_tip_with(
        &mut self,
        &(spending_input, spending_tx_hash, spent_output): &(
            &transparent::Input,
            &transaction::Hash,
            &transparent::OrderedUtxo,
        ),
    ) -> Result<(), ValidateContextError> {
        // Spending a UTXO subtracts value from the balance
        self.balance =
            (self.balance - spent_output.utxo.output.value().constrain().unwrap()).unwrap();

        let spent_outpoint = spending_input.outpoint().expect("checked by caller");

        let spent_output_tx_loc = transaction_location(spent_output);
        let output_location = OutputLocation::from_outpoint(spent_output_tx_loc, &spent_outpoint);
        let spend_was_inserted = self.spent_utxos.insert(output_location);
        assert!(
            spend_was_inserted,
            "unexpected spent output: duplicate update or duplicate spend",
        );

        self.tx_ids.insert(*spending_tx_hash);

        Ok(())
    }

    fn revert_chain_with(
        &mut self,
        &(spending_input, spending_tx_hash, spent_output): &(
            &transparent::Input,
            &transaction::Hash,
            &transparent::OrderedUtxo,
        ),
        _position: RevertPosition,
    ) {
        self.balance =
            (self.balance + spent_output.utxo.output.value().constrain().unwrap()).unwrap();

        let spent_outpoint = spending_input.outpoint().expect("checked by caller");

        let spent_output_tx_loc = transaction_location(spent_output);
        let output_location = OutputLocation::from_outpoint(spent_output_tx_loc, &spent_outpoint);
        let spend_was_removed = self.spent_utxos.remove(&output_location);
        assert!(
            spend_was_removed,
            "unexpected revert of spent output: \
             duplicate revert, or revert of a spent output that was never updated",
        );

        let tx_id_was_removed = self.tx_ids.remove(spending_tx_hash);
        assert!(
            tx_id_was_removed,
            "unexpected revert of spending input transaction: \
             duplicate revert, or revert of an input that was never updated",
        );
    }
}

impl TransparentTransfers {
    /// Returns true if there are no transfers for this address.
    pub fn is_empty(&self) -> bool {
        self.balance == Amount::<NegativeAllowed>::zero()
            && self.tx_ids.is_empty()
            && self.created_utxos.is_empty()
            && self.spent_utxos.is_empty()
    }

    /// Returns the partial balance for this address.
    #[allow(dead_code)]
    pub fn balance(&self) -> Amount<NegativeAllowed> {
        self.balance
    }

    /// Returns the [`transaction::Hash`]es of the transactions that
    /// sent or received transparent tranfers to this address,
    /// in this partial chain, in chain order.
    ///
    /// `chain_tx_by_hash` should be the `tx_by_hash` field from the [`Chain`] containing this index.
    ///
    /// # Panics
    ///
    /// If `chain_tx_by_hash` is missing some transaction hashes from this index.
    #[allow(dead_code)]
    pub fn tx_ids(
        &self,
        chain_tx_by_hash: &HashMap<transaction::Hash, TransactionLocation>,
    ) -> BTreeMap<TransactionLocation, transaction::Hash> {
        self.tx_ids
            .distinct_elements()
            .map(|tx_hash| {
                (
                    *chain_tx_by_hash
                        .get(tx_hash)
                        .expect("all hashes are indexed"),
                    *tx_hash,
                )
            })
            .collect()
    }

    /// Returns the new transparent outputs sent to this address,
    /// in this partial chain, in chain order.
    ///
    /// Some of these outputs might already be spent.
    /// [`TransparentTransfers::spent_utxos`] returns spent UTXOs.
    #[allow(dead_code)]
    pub fn created_utxos(&self) -> &BTreeMap<OutputLocation, transparent::Output> {
        &self.created_utxos
    }

    /// Returns the [`OutputLocation`]s of the spent transparent outputs sent to this address,
    /// in this partial chain, in chain order.
    #[allow(dead_code)]
    pub fn spent_utxos(&self) -> &BTreeSet<OutputLocation> {
        &self.spent_utxos
    }
}

impl Default for TransparentTransfers {
    fn default() -> Self {
        Self {
            balance: Amount::zero(),
            tx_ids: Default::default(),
            created_utxos: Default::default(),
            spent_utxos: Default::default(),
        }
    }
}

/// Returns the transaction location for an [`OrderedUtxo`].
pub fn transaction_location(ordered_utxo: &transparent::OrderedUtxo) -> TransactionLocation {
    TransactionLocation::from_usize(ordered_utxo.utxo.height, ordered_utxo.tx_index_in_block)
}
