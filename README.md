# predict-rust-sdk

An unofficial, strategy-neutral Rust client for the Predict.fun REST API and
signed LIMIT orders.

> Predict's API is beta. Pin an exact Git revision, test against synthetic or
> isolated accounts, and review upstream API changes before trading live.

## What v0.1 contains

- typed category, market, outcome, and order responses;
- strict label-to-token binding with no array-order fallback;
- EOA and Predict Account JWT authentication;
- official LIMIT amount rounding and BNB-mainnet EIP-712 domains;
- post-only signed order preparation;
- create, query-by-hash, list, and fast-remove REST calls;
- rate-limit header parsing and retry classification;
- redacted `Debug` output for API keys, JWTs, and signed payloads.

On-chain approvals, on-chain cancellation, position redemption, merge, and
WebSocket subscriptions are intentionally outside this first release.

## Example

```rust,no_run
use ethers_core::types::U256;
use predict_rust_sdk::{
    decimal_to_wei, AccountSigner, LimitOrderParams, OrderBuilder, PredictClient,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let api_key = std::env::var("PREDICT_API_KEY")?;
let private_key = std::env::var("PREDICT_ACCOUNT_KEY")?;

let client = PredictClient::mainnet(api_key)?;
let signer = AccountSigner::bnb_mainnet_eoa(&private_key)?;
let jwt = client.authenticate(&signer).await?.data;
let builder = OrderBuilder::new(signer);

let mut params = LimitOrderParams::post_only_buy(
    "12345678901234567890",
    0,
    false,
    false,
    decimal_to_wei("0.01")?,
    decimal_to_wei("230")?,
    2_000_000_000,
);
params.salt = Some(U256::from(7));

let order = builder.prepare_limit_order(&params)?;
let known_hash = order.order_hash().to_string();
match client.submit_order(&jwt, &order).await {
    Ok(response) => println!("accepted: {:?}", response.data.order_id),
    Err(error) if error.is_retryable() => {
        // The submit result is unknown. Reconcile `known_hash` before retrying.
        let existing = client.get_order_by_hash(&jwt, &known_hash).await;
        println!("reconciled: {}", existing.is_ok());
    }
    Err(error) => return Err(error.into()),
}
# Ok(())
# }
```

Never log environment-variable values, `AccessToken`, or
`PreparedOrder::expose_payload()` in production.

## Compatibility sources

The signing and amount rules track Predict's official TypeScript SDK and the
published API documentation:

- [Predict developer documentation](https://dev.predict.fun/)
- [Official TypeScript SDK](https://github.com/PredictDotFun/sdk)
- [Create-order endpoint](https://dev.predict.fun/create-an-order-32534694e0)
- [Get-order-by-hash endpoint](https://dev.predict.fun/get-order-by-hash-25326901e0)

## License

MIT. This project is not affiliated with or endorsed by Predict.fun.
