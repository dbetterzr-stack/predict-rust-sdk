//! Unofficial, strategy-neutral Rust primitives for Predict.fun.
//!
//! The crate deliberately separates deterministic order preparation from HTTP
//! submission. Callers therefore know an order hash before the network request
//! and can reconcile an ambiguous submit with [`PredictClient::get_order_by_hash`].

mod client;
mod error;
mod signing;
mod types;

pub use client::{AccessToken, ApiResponse, PredictClient, RateLimitSnapshot};
pub use error::{Error, Result};
pub use signing::{
    decimal_to_wei, AccountSigner, LimitOrderParams, OrderBuilder, OrderSide, PreparedOrder,
};
pub use types::{
    BinaryOutcomes, CancelOrdersData, Category, CreateOrderData, Market, OpenOrder, OrderStatus,
    Outcome, Page, SignedOrder, TradingStatus,
};

pub const MAINNET_BASE_URL: &str = "https://api.predict.fun/v1";
