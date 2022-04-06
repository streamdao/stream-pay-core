use solana_client::rpc_client::RpcClient;
use stream_pay_core as core;
use solana_program::pubkey::Pubkey;

use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;

mod test_helpers;
use test_helpers::{RPC_ENDPOINT, TESTNET_KEY};
use std::iter::zip;

fn send_transaction(sender: &Keypair, receiver: &Pubkey, amount: f64) {
    let initial_balance = core::get_balance(RPC_ENDPOINT, &sender.pubkey().to_string()).unwrap();
    let core::PreparedTransaction { message, fee: _ } = core::create_transaction(
        RPC_ENDPOINT,
        &sender.pubkey(), 
        amount, 
        &receiver).expect("Failed to prepare transaction");

    let _signature = core::finish_transaction(RPC_ENDPOINT, sender, message).expect("Failed to finish transaction");

    let mut check_balance_attempts = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));

        let new_balance = core::get_balance(RPC_ENDPOINT, &sender.pubkey().to_string()).unwrap();
        if new_balance != initial_balance {
            break new_balance;
        }

        check_balance_attempts += 1;

        if check_balance_attempts > 20 {
            panic!("Failed to get updated balance");
        }
    };
}

/// Makes two transactions to an arbitrary wallet and validates that `process_transaction_history` captures that history correctly.
#[test]
fn main() {
    let sender = Keypair::from_bytes(&*TESTNET_KEY).unwrap();
    let sender_pubkey = sender.pubkey();
    let random_recipient = Keypair::new().pubkey();
    let initial_balance = core::get_balance(RPC_ENDPOINT, &sender_pubkey.to_string()).unwrap();
    let min_sol_for_test = 0.05;
    assert!(initial_balance > min_sol_for_test, "Test requires more sol to run. please run `solana --url=testnet --keypair=testnet_key.json airdrop {:?}` prior to running test", min_sol_for_test);
    let mut amounts = vec![0.01, 0.02];
    let limit = (&amounts.len()).clone();
    for amount in &amounts { 
        send_transaction(&sender, &random_recipient, *amount);
    }

    let rpc_client = RpcClient::new(RPC_ENDPOINT.to_string());
    let history = core::process_transaction_history(
        &rpc_client,
        &sender_pubkey,
        None,
        None,
        limit,
    ).unwrap().into_iter().map(|confirmed_transaction| {
        confirmed_transaction.transaction
    }).collect::<Vec<_>>();
    assert_eq!(history.len(), limit);
    amounts.reverse();
    for (transaction_with_data, amount) in zip(history.clone(), amounts) {
        let transaction = transaction_with_data.transaction.decode().unwrap();
        assert!(transaction.message.account_keys.len() >= 2);
        assert_eq!(transaction.message.account_keys[0], sender_pubkey);
        assert_eq!(transaction.message.account_keys[1], random_recipient);
        let metadata = transaction_with_data.meta.unwrap();
        let sender_pre_amount = &metadata.pre_balances[0];
        let receiver_pre_amount = &metadata.pre_balances[1];
        let sender_post_amount = &metadata.post_balances[0];
        let receiver_post_amount = &metadata.post_balances[1];

        println!(
            "{:?} sent {:?} {:?} SOL with a {:?} fee", 
            sender_pubkey,
            random_recipient,
            receiver_post_amount-receiver_pre_amount,
            &metadata.fee
        );
        assert_eq!(receiver_post_amount-receiver_pre_amount, core::sol_to_lamports(amount));
        assert_eq!(sender_pre_amount-sender_post_amount - &metadata.fee, core::sol_to_lamports(amount));
    }
}
