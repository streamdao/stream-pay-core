pub use solana_client::rpc_client::{RpcClient, GetConfirmedSignaturesForAddress2Config};
use solana_client::blockhash_query::BlockhashQuery;
pub use solana_program::pubkey::Pubkey;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::signature::Signature;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    derivation_path::DerivationPath,
    hash::Hash,
    message::Message,
    signature::Signer,
    system_instruction,
    transaction::Transaction
};
pub use solana_sdk::native_token::{lamports_to_sol, sol_to_lamports};
pub use solana_sdk::signature::Keypair;
use thiserror::Error;
use spl_memo::id;

use solana_sdk::instruction::Instruction;

use std::str::FromStr;
use solana_transaction_status::{UiTransactionEncoding, EncodedConfirmedTransactionWithStatusMeta};
pub use solana_transaction_status::Encodable;
pub use solana_sdk::signature::keypair_from_seed_phrase_and_passphrase;

/// Returns the SOL balance of the given wallet address.
pub fn get_balance(rpc_endpoint: &str, base58_pubkey: &str) -> Result<f64, String> {
    let rpc_client = RpcClient::new(rpc_endpoint.to_string());

    match Pubkey::from_str(base58_pubkey) {
        Ok(pubkey) => {
            let balance_result = rpc_client.get_balance(&pubkey);
            match balance_result {
                Ok(balance) => Ok(lamports_to_sol(balance)),
                Err(e) => Err(format!("Error fetching from RPC client: {}", e)),
            }
        },
        Err(e) => Err(format!("Error getting pub key: {}", e)),
    }
}

/// Returns the history of each transaction in order from latest to earliest.
pub fn process_transaction_history(
    rpc_client: &RpcClient,
    address: &Pubkey,
    before: Option<Signature>,
    until: Option<Signature>,
    limit: usize,
) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, Box<dyn std::error::Error>> {
    let results = rpc_client.get_signatures_for_address_with_config(
        address,
        GetConfirmedSignaturesForAddress2Config {
            before,
            until,
            limit: Some(limit),
            commitment: Some(CommitmentConfig::confirmed()),
        },
    )?;
    Ok(results.into_iter().map(|result|
        rpc_client.get_transaction_with_config(
            &result.signature.parse::<Signature>().map_err(|op| format!("Unable to parse signature: Err({:?})", op))?,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
            },
        ).map_err(|op| format!("Unable to get transaction: Err({:?})", op))
    ).collect::<Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, String>>()?)
}

pub struct PreparedTransaction {
    pub message: Message,
    pub fee: f64,
}

/// Prepares a transaction to send `amount` SOL to `recipient`'s wallet address. The transaction is
/// initiated from `sender`'s address. The transaction can later be finished by
/// `finish_transaction`.
pub fn create_transaction(rpc_endpoint: &str, sender: &Pubkey, amount: f64, recipient: &Pubkey) -> Result<PreparedTransaction, String> {
    let rpc_client = RpcClient::new(rpc_endpoint.to_string());
    let spend_amount = SpendAmount::Some(sol_to_lamports(amount));
    let memo = None;

    let (message, fee) = prepare_transfer(&rpc_client, sender, spend_amount, recipient, memo).map_err(|e| format!("Error when preparing transfer: {}", e))?;

    Ok(PreparedTransaction {
        message,
        fee: lamports_to_sol(fee),
    })
}

/// Signs and executes a transaction previously created by `create_transaction`.
/// Returns the signature of the transaction if successful.
pub fn finish_transaction(rpc_endpoint: &str, private_key: &Keypair, message: Message) -> Result<String, String> {
    let rpc_client = RpcClient::new(rpc_endpoint.to_string());
    let no_wait = true;

    let res = sign_and_process_transaction(&rpc_client, private_key, no_wait, message).map_err(|e| format!("Error when finishing transaction: {}", e))?;

    Ok(format!("{:?}", res))
}

/// Converts a base58-encoded private key to its associated public key.
pub fn private_key_to_pubkey(private_key: &str) -> String {
    let keypair = Keypair::from_base58_string(private_key);
    keypair.pubkey().to_string()
}

trait WithMemo {
    fn with_memo<T: AsRef<str>>(self, memo: Option<T>) -> Self;
}

impl WithMemo for Vec<Instruction> {
    fn with_memo<T: AsRef<str>>(mut self, memo: Option<T>) -> Self {
        if let Some(memo) = &memo {
            let memo = memo.as_ref();
            let memo_ix = Instruction {
                program_id: Pubkey::new(&id().to_bytes()),
                accounts: vec![],
                data: memo.as_bytes().to_vec(),
            };
            self.push(memo_ix);
        }
        self
    }
}

fn get_fee_for_messages(
    rpc_client: &RpcClient,
    messages: &[&Message],
) -> Result<u64, CliError> {
    Ok(messages
        .iter()
        .map(|message| {
            rpc_client.get_fee_for_message(message)
        })
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .sum())
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum SpendAmount {
    All,
    Some(u64),
}

impl Default for SpendAmount {
    fn default() -> Self {
        Self::Some(u64::default())
    }
}

impl SpendAmount {
    pub fn new(amount: Option<u64>, sign_only: bool) -> Self {
        match amount {
            Some(lamports) => Self::Some(lamports),
            None if !sign_only => Self::All,
            _ => panic!("ALL amount not supported for sign-only operations"),
        }
    }
}

struct SpendAndFee {
    spend: u64,
    fee: u64,
}

fn resolve_spend_message<F>(
    rpc_client: &RpcClient,
    amount: SpendAmount,
    blockhash: Option<&Hash>,
    from_balance: u64,
    from_pubkey: &Pubkey,
    fee_pubkey: &Pubkey,
    build_message: F,
) -> Result<(Message, SpendAndFee), CliError>
where
    F: Fn(u64) -> Message,
{
    let fee = match blockhash {
        Some(blockhash) => {
            let mut dummy_message = build_message(0);
            dummy_message.recent_blockhash = *blockhash;
            get_fee_for_messages(rpc_client, &[&dummy_message])?
        }
        None => 0, // Offline, cannot calculate fee
    };

    match amount {
        SpendAmount::Some(lamports) => Ok((
            build_message(lamports),
            SpendAndFee {
                spend: lamports,
                fee,
            },
        )),
        SpendAmount::All => {
            let lamports = if from_pubkey == fee_pubkey {
                from_balance.saturating_sub(fee)
            } else {
                from_balance
            };
            Ok((
                build_message(lamports),
                SpendAndFee {
                    spend: lamports,
                    fee,
                },
            ))
        }
    }
}

/*fn check_account_for_balance_with_commitment(
    rpc_client: &RpcClient,
    account_pubkey: &Pubkey,
    balance: u64,
    commitment: CommitmentConfig,
) -> solana_client::client_error::Result<bool> {
    let lamports = rpc_client
        .get_balance_with_commitment(account_pubkey, commitment)?
        .value;
    if lamports != 0 && lamports >= balance {
        return Ok(true);
    }
    Ok(false)
}*/

#[derive(Debug, Error)]
enum CliError {
    #[error("Bad parameter: {0}")]
    BadParameter(String),
    #[error(transparent)]
    ClientError(#[from] solana_client::client_error::ClientError),
    #[error("Command not recognized: {0}")]
    CommandNotRecognized(String),
    #[error("Account {1} has insufficient funds for fee ({0} SOL)")]
    InsufficientFundsForFee(f64, Pubkey),
    #[error("Account {1} has insufficient funds for spend ({0} SOL)")]
    InsufficientFundsForSpend(f64, Pubkey),
    #[error("Account {2} has insufficient funds for spend ({0} SOL) + fee ({1} SOL)")]
    InsufficientFundsForSpendAndFee(f64, f64, Pubkey),
    /*#[error(transparent)]
    InvalidNonce(nonce_utils::Error),*/
    #[error("Dynamic program error: {0}")]
    DynamicProgramError(String),
    #[error("RPC request error: {0}")]
    RpcRequestError(String),
    #[error("Keypair file not found: {0}")]
    KeypairFileNotFound(String),
}

impl From<Box<dyn std::error::Error>> for CliError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        CliError::DynamicProgramError(error.to_string())
    }
}

/*impl From<nonce_utils::Error> for CliError {
    fn from(error: nonce_utils::Error) -> Self {
        match error {
            nonce_utils::Error::Client(client_error) => Self::RpcRequestError(client_error),
            _ => Self::InvalidNonce(error),
        }
    }
}*/

fn resolve_spend_tx_and_check_account_balances<F>(
    rpc_client: &RpcClient,
    sign_only: bool,
    amount: SpendAmount,
    blockhash: &Hash,
    from_pubkey: &Pubkey,
    fee_pubkey: &Pubkey,
    build_message: F,
    commitment: CommitmentConfig,
) -> Result<(Message, SpendAndFee), CliError>
where
    F: Fn(u64) -> Message,
{
    if sign_only {
        unreachable!()
        /*let (message, SpendAndFee { spend, fee: _ }) = resolve_spend_message(
            rpc_client,
            amount,
            None,
            0,
            from_pubkey,
            fee_pubkey,
            build_message,
        )?;
        Ok((message, spend))*/
    } else {
        let from_balance = rpc_client
            .get_balance_with_commitment(from_pubkey, commitment)?
            .value;
        let (message, cost) = resolve_spend_message(
            rpc_client,
            amount,
            Some(blockhash),
            from_balance,
            from_pubkey,
            fee_pubkey,
            build_message,
        )?;
        let spend = cost.spend;
        let fee = cost.fee;
        if from_pubkey == fee_pubkey {
            if from_balance == 0 || from_balance < spend + fee {
                return Err(CliError::InsufficientFundsForSpendAndFee(
                    lamports_to_sol(spend),
                    lamports_to_sol(fee),
                    *from_pubkey,
                ));
            }
        } else {
            unreachable!()
            /*if from_balance < spend {
                return Err(CliError::InsufficientFundsForSpend(
                    lamports_to_sol(spend),
                    *from_pubkey,
                ));
            }
            if !check_account_for_balance_with_commitment(rpc_client, fee_pubkey, fee, commitment)?
            {
                return Err(CliError::InsufficientFundsForFee(
                    lamports_to_sol(fee),
                    *fee_pubkey,
                ));
            }*/
        }
        Ok((message, cost))
    }
}

type PrepareTransferResult = Result<(Message, u64), Box<dyn std::error::Error>>;
type ProcessResult = Result<String, Box<dyn std::error::Error>>;

fn prepare_transfer(
    rpc_client: &RpcClient,
    //config: &CliConfig,
    //commitment: &CommitmentConfig,
    sender: &Pubkey,
    amount: SpendAmount,
    to: &Pubkey,
    //sign_only: bool,
    //blockhash_query: &BlockhashQuery,
    //nonce_account: Option<&Pubkey>,
    memo: Option<&String>,
    //derived_address_seed: Option<String>,
    //derived_address_program_id: Option<&Pubkey>,
) -> PrepareTransferResult {
    let sign_only = false;

    let from_pubkey = sender;
    //let nonce_account = None;
    let fee_payer = sender;

    let blockhash = None;
    //let nonce_account = Some(sender.pubkey());
    let nonce_account = None;
    let blockhash_query = BlockhashQuery::new(blockhash, sign_only, nonce_account);

    let commitment = CommitmentConfig::finalized();

    // TODO - get_recent_blockhash is deprecated on v1.9, but the replacement get_latest_blockhash
    // doesn't work on mainnet RPC providers.
    //let recent_blockhash = blockhash_query.get_blockhash(rpc_client, commitment)?;
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;

    /*let derived_parts = derived_address_seed.zip(derived_address_program_id);
    let with_seed = if let Some((seed, program_id)) = derived_parts {
        let base_pubkey = from_pubkey;
        from_pubkey = Pubkey::create_with_seed(&base_pubkey, &seed, program_id)?;
        Some((base_pubkey, seed, program_id, from_pubkey))
    } else {
        None
    };*/

    let build_message = |lamports| {
        let ixs = /*if let Some((base_pubkey, seed, program_id, from_pubkey)) = with_seed.as_ref() {
            vec![system_instruction::transfer_with_seed(
                from_pubkey,
                base_pubkey,
                seed.clone(),
                program_id,
                to,
                lamports,
            )]
            .with_memo(memo)
        } else {*/
            vec![system_instruction::transfer(&from_pubkey, to, lamports)].with_memo(memo)
        /*}*/;

        if let Some(nonce_account) = &nonce_account {
            unreachable!()
            /*Message::new_with_nonce(
                ixs,
                Some(&fee_payer.pubkey()),
                nonce_account,
                &nonce_authority.pubkey(),
            )*/
        } else {
            Message::new_with_blockhash(&ixs, Some(&fee_payer), &recent_blockhash)
        }
    };

    let (message, cost) = resolve_spend_tx_and_check_account_balances(
        rpc_client,
        sign_only,
        amount,
        &recent_blockhash,
        &from_pubkey,
        &fee_payer,
        build_message,
        commitment,
    )?;

    Ok((message, cost.fee))
}

fn sign_and_process_transaction(
    rpc_client: &RpcClient,
    sender: &dyn Signer,
    //sign_only: bool,
    //dump_transaction_message: bool,
    no_wait: bool,
    //nonce_account: Option<&Pubkey>,
    message: Message,
) -> ProcessResult {
    let sign_only = false;
    let nonce_account: Option<&Pubkey> = None;

    let recent_blockhash = message.recent_blockhash.clone();

    let mut tx = Transaction::new_unsigned(message);

    let signers = vec![sender];

    if sign_only {
        tx.try_partial_sign(&signers, recent_blockhash)?;
        /*return_signers_with_config(
            &tx,
            &config.output_format,
            &ReturnSignersConfig {
                dump_transaction_message,
            },
        )*/
        Ok("".to_string())
    } else {
        if let Some(nonce_account) = &nonce_account {
            unreachable!()
            /*let nonce_account = nonce_utils::get_account_with_commitment(
                rpc_client,
                nonce_account,
                commitment,
            )?;
            check_nonce_account(&nonce_account, &nonce_authority.pubkey(), &recent_blockhash)?;*/
        }

        tx.try_sign(&signers, recent_blockhash)?;
        let signature = if no_wait {
            rpc_client.send_transaction(&tx)
        } else {
            rpc_client.send_and_confirm_transaction_with_spinner(&tx)
        }?;

        Ok(signature.to_string())
    }
}