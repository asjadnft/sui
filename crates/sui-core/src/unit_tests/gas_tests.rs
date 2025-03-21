// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

use super::authority_tests::{init_state_with_ids, send_and_confirm_transaction};
use super::move_integration_tests::build_and_try_publish_test_package;
use crate::authority::authority_tests::{init_state, init_state_with_ids_and_object_basics};
use move_core_types::account_address::AccountAddress;
use move_core_types::ident_str;
use once_cell::sync::Lazy;
use sui_protocol_config::ProtocolConfig;
use sui_types::crypto::AccountKeyPair;
use sui_types::gas::SuiCostTable;
use sui_types::gas_coin::GasCoin;
use sui_types::object::GAS_VALUE_FOR_TESTING;
use sui_types::programmable_transaction_builder::ProgrammableTransactionBuilder;
use sui_types::utils::to_sender_signed_transaction;
use sui_types::{base_types::dbg_addr, crypto::get_key_pair};

static MAX_GAS_BUDGET: Lazy<u64> = Lazy::new(|| SuiCostTable::new_for_testing().max_gas_budget());
static MIN_GAS_BUDGET: Lazy<u64> = Lazy::new(|| SuiCostTable::new_for_testing().min_gas_budget());

#[tokio::test]
async fn test_tx_less_than_minimum_gas_budget() {
    // This test creates a transaction that sets a gas_budget less than the minimum
    // transaction requirement. It's expected to fail early during transaction
    // handling phase.
    let budget = *MIN_GAS_BUDGET - 1;
    let result = execute_transfer(*MAX_GAS_BUDGET, budget, false).await;

    assert_eq!(
        UserInputError::try_from(result.response.unwrap_err()).unwrap(),
        UserInputError::GasBudgetTooLow {
            gas_budget: budget,
            min_budget: *MIN_GAS_BUDGET
        }
    );
}

#[tokio::test]
async fn test_tx_more_than_maximum_gas_budget() {
    // This test creates a transaction that sets a gas_budget more than the maximum
    // budget (which could lead to overflow). It's expected to fail early during transaction
    // handling phase.
    let budget = *MAX_GAS_BUDGET + 1;
    let result = execute_transfer(*MAX_GAS_BUDGET, budget, false).await;

    assert_eq!(
        UserInputError::try_from(result.response.unwrap_err()).unwrap(),
        UserInputError::GasBudgetTooHigh {
            gas_budget: budget,
            max_budget: *MAX_GAS_BUDGET
        }
    );
}

// #[tokio::test]
// async fn test_tx_max_computation() -> SuiResult {
//     let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
//     let gas_object_id = ObjectID::random();
//     let authority_state = init_state_with_ids(vec![(sender, gas_object_id)]).await;
//
//     let response = build_and_try_publish_test_package(
//         &authority_state,
//         &sender,
//         &sender_key,
//         &gas_object_id,
//         "move_random",
//         *MAX_GAS_BUDGET,
//         /* with_unpublished_deps */ false,
//     )
//     .await;
//     let effects = response.1.into_data();
//     assert!(effects.status().is_ok());
//     let package = effects
//         .created()
//         .iter()
//         .find(|(_, owner)| matches!(owner, Owner::Immutable))
//         .unwrap()
//         .0
//         .0;
//
//     let gas_object = authority_state.get_object(&gas_object_id).await.unwrap().unwrap();
//     let initial_value = GasCoin::try_from(&gas_object)?.value();
//     let gas_object_ref = gas_object.compute_object_reference();
//     let module = ident_str!("move_random").to_owned();
//     let function = ident_str!("loopy").to_owned();
//     let args = vec![];
//     let budget = 1_500_000_000;
//     let data = TransactionData::new_move_call(
//         sender,
//         package,
//         module,
//         function,
//         vec![],
//         gas_object_ref,
//         args,
//         budget,
//         300,
//     )
//     .unwrap();
//
//     let tx = to_sender_signed_transaction(data, &sender_key);
//     let effects = send_and_confirm_transaction(&authority_state, tx)
//         .await
//         .unwrap()
//         .1
//         .into_data();
//     assert_eq!(
//         effects.status().clone().unwrap_err().0,
//         ExecutionFailureStatus::InsufficientGas
//     );
//     let gas_ref = effects.gas_object().0;
//     let gas_object = authority_state.get_object(&gas_ref.0).await.unwrap().unwrap();
//     let final_value = GasCoin::try_from(&gas_object)?.value();
//     let summary = effects.gas_cost_summary();
//     assert!(summary.computation_cost == budget);
//     assert!(initial_value - budget == final_value);
//     Ok(())
// }

#[tokio::test]
async fn test_tx_gas_balance_less_than_budget() {
    // This test creates a transaction that uses a gas object whose balance
    // is not even enough to pay for the gas budget. This should fail early
    // during handle transaction phase.
    let gas_balance = *MIN_GAS_BUDGET - 1;
    let budget = *MIN_GAS_BUDGET;
    let result = execute_transfer_with_price(gas_balance, budget, 1, false).await;
    assert!(matches!(
        UserInputError::try_from(result.response.unwrap_err()).unwrap(),
        UserInputError::GasBalanceTooLow { .. }
    ));
}

#[tokio::test]
async fn test_native_transfer_sufficient_gas() -> SuiResult {
    // This test does a native transfer with sufficient gas budget and balance.
    // It's expected to succeed. We check that gas was charged properly.
    let result = execute_transfer(*MAX_GAS_BUDGET, *MAX_GAS_BUDGET, true).await;
    let effects = result
        .response
        .unwrap()
        .into_effects_for_testing()
        .into_data();
    let gas_cost = effects.gas_cost_summary();
    assert!(gas_cost.net_gas_usage() as u64 > *MIN_GAS_BUDGET);
    assert!(gas_cost.computation_cost > 0);
    assert!(gas_cost.storage_cost > 0);
    // Removing genesis object does not have rebate.
    assert_eq!(gas_cost.storage_rebate, 0);

    let gas_object = result
        .authority_state
        .get_object(&result.gas_object_id)
        .await?
        .unwrap();
    assert_eq!(
        GasCoin::try_from(&gas_object)?.value(),
        *MAX_GAS_BUDGET - gas_cost.gas_used()
    );
    Ok(())
}

#[tokio::test]
async fn test_native_transfer_gas_price_is_used() {
    let result = execute_transfer_with_price(*MAX_GAS_BUDGET, *MAX_GAS_BUDGET, 1, true).await;
    let effects = result
        .response
        .unwrap()
        .into_effects_for_testing()
        .into_data();
    let gas_summary_1 = effects.gas_cost_summary();

    let result = execute_transfer_with_price(*MAX_GAS_BUDGET, *MAX_GAS_BUDGET, 2, true).await;
    let effects = result
        .response
        .unwrap()
        .into_effects_for_testing()
        .into_data();
    let gas_summary_2 = effects.gas_cost_summary();

    assert_eq!(
        gas_summary_1.computation_cost * 2,
        gas_summary_2.computation_cost
    );

    // test overflow with insufficient gas
    let gas_balance = *MAX_GAS_BUDGET - 1;
    let gas_budget = *MAX_GAS_BUDGET;
    let result = execute_transfer_with_price(gas_balance, gas_budget, 1, true).await;
    assert!(matches!(
        UserInputError::try_from(result.response.unwrap_err()).unwrap(),
        UserInputError::GasBalanceTooLow { .. }
    ));
}

#[tokio::test]
async fn test_transfer_sui_insufficient_gas() {
    let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
    let recipient = dbg_addr(2);
    let gas_object_id = ObjectID::random();
    let gas_object = Object::with_id_owner_gas_for_testing(gas_object_id, sender, *MIN_GAS_BUDGET);
    let gas_object_ref = gas_object.compute_object_reference();
    let authority_state = init_state().await;
    authority_state.insert_genesis_object(gas_object).await;

    let pt = {
        let mut builder = ProgrammableTransactionBuilder::new();
        builder.transfer_sui(recipient, None);
        builder.finish()
    };
    let kind = TransactionKind::ProgrammableTransaction(pt);
    let data = TransactionData::new(kind, sender, gas_object_ref, *MIN_GAS_BUDGET, 1);
    let tx = to_sender_signed_transaction(data, &sender_key);

    let effects = send_and_confirm_transaction(&authority_state, tx)
        .await
        .unwrap()
        .1
        .into_data();
    // We expect this to fail due to insufficient gas.
    assert_eq!(
        *effects.status(),
        ExecutionStatus::new_failure(ExecutionFailureStatus::InsufficientGas, None)
    );
    // Ensure that the owner of the object did not change if the transfer failed.
    assert_eq!(effects.mutated()[0].1, sender);
}

/// - All gas coins should be owned by an address (not shared or immutable)
/// - All gas coins should be owned by the sender, or the sponsor
#[tokio::test]
async fn test_invalid_gas_owners() {
    let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
    let authority_state = init_state().await;

    let init_object = |o: Object| async {
        let obj_ref = o.compute_object_reference();
        authority_state.insert_genesis_object(o).await;
        obj_ref
    };

    let gas_object1 = init_object(Object::with_owner_for_testing(sender)).await;
    let gas_object2 = init_object(Object::with_owner_for_testing(sender)).await;
    let gas_object3 = init_object(Object::with_owner_for_testing(sender)).await;
    let gas_object4 = init_object(Object::with_owner_for_testing(sender)).await;

    let shared_object = init_object(Object::shared_for_testing()).await;
    let immutable_object = init_object(Object::immutable_for_testing()).await;
    let id_owned_object = init_object(Object::with_object_owner_for_testing(
        ObjectID::random(),
        gas_object3.0,
    ))
    .await;
    let non_sender_owned_object =
        init_object(Object::with_owner_for_testing(SuiAddress::ZERO)).await;

    async fn test(
        good_gas_object: ObjectRef,
        bad_gas_object: ObjectRef,
        sender: SuiAddress,
        sender_key: &AccountKeyPair,
        authority_state: &AuthorityState,
    ) -> UserInputError {
        let pt = {
            let mut builder = ProgrammableTransactionBuilder::new();
            let recipient = dbg_addr(2);
            builder.transfer_sui(recipient, None);
            builder.finish()
        };
        let kind = TransactionKind::ProgrammableTransaction(pt);
        let data = TransactionData::new_with_gas_coins(
            kind,
            sender,
            vec![good_gas_object, bad_gas_object],
            *MAX_GAS_BUDGET,
            1,
        );
        let tx = to_sender_signed_transaction(data, sender_key);

        let result = send_and_confirm_transaction(authority_state, tx).await;
        UserInputError::try_from(result.unwrap_err()).unwrap()
    }

    assert_eq!(
        test(
            gas_object1,
            shared_object,
            sender,
            &sender_key,
            &authority_state
        )
        .await,
        UserInputError::GasObjectNotOwnedObject {
            owner: Owner::Shared {
                initial_shared_version: OBJECT_START_VERSION
            }
        }
    );
    assert_eq!(
        test(
            gas_object2,
            immutable_object,
            sender,
            &sender_key,
            &authority_state
        )
        .await,
        UserInputError::GasObjectNotOwnedObject {
            owner: Owner::Immutable
        }
    );
    assert_eq!(
        test(
            gas_object3,
            id_owned_object,
            sender,
            &sender_key,
            &authority_state
        )
        .await,
        UserInputError::GasObjectNotOwnedObject {
            owner: Owner::ObjectOwner(gas_object3.0.into())
        }
    );
    assert!(matches!(
        test(
            gas_object4,
            non_sender_owned_object,
            sender,
            &sender_key,
            &authority_state
        )
        .await,
        UserInputError::IncorrectUserSignature { .. }
    ))
}

#[tokio::test]
async fn test_native_transfer_insufficient_gas_reading_objects() {
    // This test creates a transfer transaction with a gas budget, that's more than
    // the minimum budget requirement, but not enough to even read the objects from db.
    // This will lead to failure in lock check step during handle transaction phase.
    let balance = *MIN_GAS_BUDGET + 1;
    let result = execute_transfer(balance, balance, true).await;
    // The transaction should still execute to effects, but with execution status as failure.
    let effects = result
        .response
        .unwrap()
        .into_effects_for_testing()
        .into_data();
    assert_eq!(
        effects.into_status().unwrap_err().0,
        ExecutionFailureStatus::InsufficientGas
    );
}

#[tokio::test]
async fn test_native_transfer_insufficient_gas_execution() {
    // This test creates a transfer transaction with a gas budget that's insufficient
    // to finalize the transfer object mutation effects. It will fail during
    // execution phase, and hence gas object will still be mutated and all budget
    // will be charged.
    let result = execute_transfer(*MAX_GAS_BUDGET, *MAX_GAS_BUDGET, true).await;
    let total_gas = result
        .response
        .unwrap()
        .into_effects_for_testing()
        .data()
        .gas_cost_summary()
        .gas_used();
    let budget = total_gas - 1;
    let result = execute_transfer(budget, budget, true).await;
    let effects = result
        .response
        .unwrap()
        .into_effects_for_testing()
        .into_data();
    // Transaction failed for out of gas so charge is same as budget
    assert!(effects.gas_cost_summary().gas_used() == budget);
    let gas_object = result
        .authority_state
        .get_object(&result.gas_object_id)
        .await
        .unwrap()
        .unwrap();
    let gas_coin = GasCoin::try_from(&gas_object).unwrap();
    assert_eq!(gas_coin.value(), 0);
    // After a failed transfer, the version should have been incremented,
    // but the owner of the object should remain the same, unchanged.
    let ((_, version, _), owner) = effects.mutated_excluding_gas().first().unwrap();
    assert_eq!(version, &gas_object.version());
    assert_eq!(owner, &gas_object.owner);

    assert_eq!(
        effects.into_status().unwrap_err().0,
        ExecutionFailureStatus::InsufficientGas,
    );
}

#[tokio::test]
async fn test_publish_gas() -> anyhow::Result<()> {
    let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
    let gas_object_id = ObjectID::random();
    let authority_state = init_state_with_ids(vec![(sender, gas_object_id)]).await;
    let rgp = authority_state.reference_gas_price_for_testing().unwrap();

    // The successful case.
    let response = build_and_try_publish_test_package(
        &authority_state,
        &sender,
        &sender_key,
        &gas_object_id,
        "object_wrapping",
        TEST_ONLY_GAS_UNIT_FOR_PUBLISH * rgp * 2,
        rgp,
        /* with_unpublished_deps */ false,
    )
    .await;
    let effects = response.1.into_data();
    let gas_cost = effects.gas_cost_summary();
    assert!(gas_cost.storage_cost > 0);

    let gas_object = authority_state.get_object(&gas_object_id).await?.unwrap();
    let gas_size = gas_object.object_size_for_gas_metering();
    let expected_gas_balance = GAS_VALUE_FOR_TESTING - gas_cost.net_gas_usage() as u64;
    assert_eq!(
        GasCoin::try_from(&gas_object)?.value(),
        expected_gas_balance,
    );

    // Create a transaction with budget DELTA less than the gas cost required.
    let total_gas_used = gas_cost.net_gas_usage() as u64;
    let config = ProtocolConfig::get_for_max_version();
    let delta: u64 =
        gas_size as u64 * config.obj_data_cost_refundable() * config.storage_gas_price() + 1000;
    let budget = if delta < total_gas_used {
        total_gas_used - delta
    } else {
        total_gas_used - 10
    };
    // Run the transaction again with 1 less than the required budget.
    let response = build_and_try_publish_test_package(
        &authority_state,
        &sender,
        &sender_key,
        &gas_object_id,
        "object_wrapping",
        budget,
        rgp,
        /* with_unpublished_deps */ false,
    )
    .await;
    let effects = response.1.into_data();
    let gas_cost = effects.gas_cost_summary().clone();
    let err = effects.into_status().unwrap_err().0;

    assert_eq!(err, ExecutionFailureStatus::InsufficientGas);

    // Make sure that we are not charging storage cost at failure.
    assert_eq!(gas_cost.storage_cost, 0);
    assert_eq!(gas_cost.storage_rebate, 0);
    // Upon OOG failure, we should charge the whole budget
    assert_eq!(gas_cost.gas_used(), budget);

    let gas_object = authority_state.get_object(&gas_object_id).await?.unwrap();
    let expected_gas_balance = expected_gas_balance - gas_cost.gas_used();
    assert_eq!(
        GasCoin::try_from(&gas_object)?.value(),
        expected_gas_balance,
    );

    Ok(())
}

#[tokio::test]
async fn test_move_call_gas() -> SuiResult {
    let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
    let gas_object_id = ObjectID::random();
    let (authority_state, package_object_ref) =
        init_state_with_ids_and_object_basics(vec![(sender, gas_object_id)]).await;
    let rgp = authority_state.reference_gas_price_for_testing().unwrap();
    let gas_object = authority_state.get_object(&gas_object_id).await?.unwrap();

    let module = ident_str!("object_basics").to_owned();
    let function = ident_str!("create").to_owned();
    let args = vec![
        CallArg::Pure(16u64.to_le_bytes().to_vec()),
        CallArg::Pure(bcs::to_bytes(&AccountAddress::from(sender)).unwrap()),
    ];
    let data = TransactionData::new_move_call(
        sender,
        package_object_ref.0,
        module.clone(),
        function.clone(),
        Vec::new(),
        gas_object.compute_object_reference(),
        args.clone(),
        *MAX_GAS_BUDGET,
        rgp,
    )
    .unwrap();

    let tx = to_sender_signed_transaction(data, &sender_key);
    let response = send_and_confirm_transaction(&authority_state, tx).await?;
    let effects = response.1.into_data();
    let created_object_ref = effects.created()[0].0;
    assert!(effects.status().is_ok());
    let gas_cost = effects.gas_cost_summary();
    assert!(gas_cost.storage_cost > 0);
    assert_eq!(gas_cost.storage_rebate, 0);
    let gas_object = authority_state.get_object(&gas_object_id).await?.unwrap();
    let expected_gas_balance = GAS_VALUE_FOR_TESTING - gas_cost.net_gas_usage() as u64;
    assert_eq!(
        GasCoin::try_from(&gas_object)?.value(),
        expected_gas_balance,
    );

    // This is the total amount of storage cost paid. We will use this
    // to check if we get back the same amount of rebate latter.
    let prev_storage_cost = gas_cost.storage_cost;

    // Execute object deletion, and make sure we have storage rebate.
    let data = TransactionData::new_move_call(
        sender,
        package_object_ref.0,
        module.clone(),
        ident_str!("delete").to_owned(),
        vec![],
        gas_object.compute_object_reference(),
        vec![CallArg::Object(ObjectArg::ImmOrOwnedObject(
            created_object_ref,
        ))],
        *MAX_GAS_BUDGET,
        rgp,
    )
    .unwrap();

    let transaction = to_sender_signed_transaction(data, &sender_key);
    let response = send_and_confirm_transaction(&authority_state, transaction).await?;
    let effects = response.1.into_data();
    assert!(effects.status().is_ok());
    let gas_cost = effects.gas_cost_summary();
    // storage_cost should be less than rebate because for object deletion, we only
    // rebate without charging.
    assert!(gas_cost.storage_cost > 0 && gas_cost.storage_cost < gas_cost.storage_rebate);
    // Check that we have storage rebate is less or equal to the previous one + non refundable
    assert_eq!(
        gas_cost.storage_rebate + gas_cost.non_refundable_storage_fee,
        prev_storage_cost
    );
    Ok(())
}

#[tokio::test]
async fn test_tx_gas_price_less_than_reference_gas_price() {
    let gas_balance = *MAX_GAS_BUDGET;
    let budget = *MIN_GAS_BUDGET;
    let result = execute_transfer_with_price(gas_balance, budget, 0, false).await;
    assert!(matches!(
        UserInputError::try_from(result.response.unwrap_err()).unwrap(),
        UserInputError::GasPriceUnderRGP { .. }
    ));
}

struct TransferResult {
    pub authority_state: Arc<AuthorityState>,
    pub gas_object_id: ObjectID,
    pub response: SuiResult<TransactionStatus>,
}

async fn execute_transfer(gas_balance: u64, gas_budget: u64, run_confirm: bool) -> TransferResult {
    execute_transfer_with_price(gas_balance, gas_budget, 1, run_confirm).await
}

async fn execute_transfer_with_price(
    gas_balance: u64,
    gas_budget: u64,
    rgp_multiple: u64,
    run_confirm: bool,
) -> TransferResult {
    let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
    let object_id: ObjectID = ObjectID::random();
    let recipient = dbg_addr(2);
    let authority_state = init_state_with_ids(vec![(sender, object_id)]).await;
    let rgp = authority_state.reference_gas_price_for_testing().unwrap() * rgp_multiple;
    let epoch_store = authority_state.load_epoch_store_one_call_per_task();
    let gas_object_id = ObjectID::random();
    let gas_object = Object::with_id_owner_gas_for_testing(gas_object_id, sender, gas_balance);
    let gas_object_ref = gas_object.compute_object_reference();
    authority_state.insert_genesis_object(gas_object).await;
    let object = authority_state
        .get_object(&object_id)
        .await
        .unwrap()
        .unwrap();

    let pt = {
        let mut builder = ProgrammableTransactionBuilder::new();
        builder
            .transfer_object(recipient, object.compute_object_reference())
            .unwrap();
        builder.finish()
    };
    let kind = TransactionKind::ProgrammableTransaction(pt);
    let data = TransactionData::new(kind, sender, gas_object_ref, gas_budget, rgp);
    let tx = to_sender_signed_transaction(data, &sender_key);

    let response = if run_confirm {
        send_and_confirm_transaction(&authority_state, tx)
            .await
            .map(|(cert, effects)| {
                TransactionStatus::Executed(
                    Some(cert.into_sig()),
                    effects,
                    TransactionEvents::default(),
                )
            })
    } else {
        authority_state
            .handle_transaction(&epoch_store, tx)
            .await
            .map(|r| r.status)
    };
    TransferResult {
        authority_state,
        gas_object_id,
        response,
    }
}
