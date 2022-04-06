# StreamPay Core

Core utilities used by the [StreamPay](https://streampayment.app) app.

# Testing

Install tools
```
cargo install solana-keygen
```
https://docs.solana.com/cli/install-solana-cli-tools

Generate a new keypair for testing (`testnet_key.json` is listed in `.gitignore`):

```
solana-keygen new --outfile testnet_key.json
```

Request an airdrop on the testnet:

```
solana --url=testnet --keypair=testnet_key.json airdrop 1
```

Verify balance:

```
solana --url=testnet --keypair=testnet_key.json balance
```

Then run tests:

```
cargo test
```
