use stream-pay-core as core;

use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;

mod test_helpers;
use test_helpers::{RPC_ENDPOINT, TESTNET_KEY};

/// Transfers 0.5 SOL back to the sending address, effectively decrementing the balance by the
/// transaction fee.
#[test]
fn main() {
    let sender = Keypair::from_bytes(&*TESTNET_KEY).unwrap();
    let sender_pubkey = sender.pubkey();
    let initial_balance = core::get_balance(RPC_ENDPOINT, &sender_pubkey.to_string()).unwrap();
    println!("Initial balance: {:.6}", initial_balance);

    let core::PreparedTransaction { message, fee } = core::create_transaction(RPC_ENDPOINT, &sender_pubkey, 0.5, &sender_pubkey).expect("Failed to prepare transaction");
    println!("Estimated fee: {:.6}", fee);

    let _signature = core::finish_transaction(RPC_ENDPOINT, &sender, message).expect("Failed to finish transaction");

    let mut check_balance_attempts = 0;
    let final_balance = loop {
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

    println!("Final balance: {:.6}", final_balance);
    assert_eq!(final_balance, initial_balance - fee);
}
