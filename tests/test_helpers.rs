use once_cell::sync::Lazy;

pub const RPC_ENDPOINT: &str = "https://api.testnet.solana.com";
pub const TESTNET_KEY_FILE: &str = "testnet_key.json";

pub static TESTNET_KEY: Lazy<[u8; 64]> = Lazy::new(|| {
    let key_json = std::fs::read_to_string(TESTNET_KEY_FILE).unwrap_or_else(|e| panic!("Unable to read {}: {}", TESTNET_KEY_FILE, e));
    let keypair: Vec<u8> = serde_json::from_str(&key_json).unwrap_or_else(|e| panic!("Unable to parse keypair from {}: {}", TESTNET_KEY_FILE, e));

    assert_eq!(keypair.len(), 64);

    let mut result = [0; 64];
    result.copy_from_slice(&keypair);

    result
});