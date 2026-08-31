use crate::{Error, Result, SignedOrder};
use ethers_core::{
    abi::{encode, Token},
    types::{Address, H256, U256},
    utils::keccak256,
};
use ethers_signers::{LocalWallet, Signer};
use rand::Rng;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{fmt, str::FromStr};

const WEI_DECIMALS: u32 = 18;
const MAX_SALT: u64 = 2_147_483_648;
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
const PROTOCOL_NAME: &str = "predict.fun CTF Exchange";
const PROTOCOL_VERSION: &str = "1";
const KERNEL_NAME: &str = "Kernel";
const KERNEL_VERSION: &str = "0.3.1";
const ECDSA_VALIDATOR_MAINNET: &str = "0x845ADb2C711129d4f3966735eD98a9F09fC4cE57";
const CTF_EXCHANGE_MAINNET: &str = "0x8BC070BEdAB741406F4B1Eb65A72bee27894B689";
const NEG_RISK_CTF_EXCHANGE_MAINNET: &str = "0x365fb81bd4A24D6303cd2F19c349dE6894D8d58A";
const YIELD_BEARING_CTF_EXCHANGE_MAINNET: &str = "0x6bEb5a40C032AFc305961162d8204CDA16DECFa5";
const YIELD_BEARING_NEG_RISK_CTF_EXCHANGE_MAINNET: &str =
    "0x8A289d458f5a134bA40015085A8F50Ffb681B41d";

#[derive(Clone)]
enum AccountMode {
    Eoa,
    PredictAccount(Address),
}

/// Wallet-backed signer. Its `Debug` implementation never exposes key material.
#[derive(Clone)]
pub struct AccountSigner {
    chain_id: u64,
    wallet: LocalWallet,
    mode: AccountMode,
}

impl fmt::Debug for AccountSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccountSigner")
            .field("chain_id", &self.chain_id)
            .field("signer_address", &"[REDACTED]")
            .field(
                "mode",
                &match self.mode {
                    AccountMode::Eoa => "eoa",
                    AccountMode::PredictAccount(_) => "predict_account",
                },
            )
            .finish_non_exhaustive()
    }
}

impl AccountSigner {
    pub fn bnb_mainnet_eoa(private_key: &str) -> Result<Self> {
        Self::eoa(56, private_key)
    }

    pub fn bnb_mainnet_predict_account(
        private_key: &str,
        predict_account_address: &str,
    ) -> Result<Self> {
        Self::predict_account(56, private_key, predict_account_address)
    }

    pub fn eoa(chain_id: u64, private_key: &str) -> Result<Self> {
        ensure_supported_chain(chain_id)?;
        let wallet = parse_wallet(private_key)?.with_chain_id(chain_id);
        Ok(Self {
            chain_id,
            wallet,
            mode: AccountMode::Eoa,
        })
    }

    pub fn predict_account(
        chain_id: u64,
        private_key: &str,
        predict_account_address: &str,
    ) -> Result<Self> {
        ensure_supported_chain(chain_id)?;
        let wallet = parse_wallet(private_key)?.with_chain_id(chain_id);
        let account = parse_address(predict_account_address)?;
        Ok(Self {
            chain_id,
            wallet,
            mode: AccountMode::PredictAccount(account),
        })
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn signer_address(&self) -> Address {
        self.wallet.address()
    }

    pub fn order_address(&self) -> Address {
        match self.mode {
            AccountMode::Eoa => self.wallet.address(),
            AccountMode::PredictAccount(address) => address,
        }
    }

    pub(crate) fn auth_payload(&self, message: &str) -> Result<Value> {
        let signature = match self.mode {
            AccountMode::Eoa => self.sign_digest(H256::from(eip191_text_hash(message)))?,
            AccountMode::PredictAccount(_) => {
                self.sign_predict_account_hash(H256::from(eip191_text_hash(message)))?
            }
        };
        Ok(json!({
            "signer": address_hex(self.order_address()),
            "message": message,
            "signature": signature,
        }))
    }

    fn sign_order_hash(&self, order_hash: H256) -> Result<String> {
        match self.mode {
            AccountMode::Eoa => self.sign_digest(order_hash),
            AccountMode::PredictAccount(_) => self.sign_predict_account_hash(order_hash),
        }
    }

    fn sign_predict_account_hash(&self, raw_hash: H256) -> Result<String> {
        let AccountMode::PredictAccount(predict_account) = self.mode else {
            return Err(Error::Signing(
                "Predict Account signature requested for EOA".to_string(),
            ));
        };
        let domain =
            eip712_domain_separator(KERNEL_NAME, KERNEL_VERSION, self.chain_id, predict_account);
        let digest = eip712_digest(domain, hash_kernel_message(raw_hash));
        let personal_digest = H256::from(eip191_bytes_hash(digest.as_bytes()));
        let signature = self.sign_digest(personal_digest)?;
        let validator = parse_address(ECDSA_VALIDATOR_MAINNET)?;
        Ok(format!(
            "0x01{}{}",
            hex::encode(validator.as_bytes()),
            signature.trim_start_matches("0x")
        ))
    }

    fn sign_digest(&self, digest: H256) -> Result<String> {
        self.wallet
            .sign_hash(digest)
            .map(|signature| signature.to_string().trim_start_matches("0x").to_string())
            .map_err(|_| Error::Signing("wallet rejected digest".to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    fn as_u8(self) -> u8 {
        match self {
            Self::Buy => 0,
            Self::Sell => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LimitOrderParams {
    pub token_id: String,
    pub fee_rate_bps: u64,
    pub is_neg_risk: bool,
    pub is_yield_bearing: bool,
    pub side: OrderSide,
    pub price_per_share_wei: U256,
    pub quantity_wei: U256,
    pub post_only: bool,
    pub salt: Option<U256>,
    pub nonce: U256,
    pub expires_at_seconds: u64,
}

impl LimitOrderParams {
    pub fn post_only_buy(
        token_id: impl Into<String>,
        fee_rate_bps: u64,
        is_neg_risk: bool,
        is_yield_bearing: bool,
        price_per_share_wei: U256,
        quantity_wei: U256,
        expires_at_seconds: u64,
    ) -> Self {
        Self {
            token_id: token_id.into(),
            fee_rate_bps,
            is_neg_risk,
            is_yield_bearing,
            side: OrderSide::Buy,
            price_per_share_wei,
            quantity_wei,
            post_only: true,
            salt: None,
            nonce: U256::zero(),
            expires_at_seconds,
        }
    }
}

/// Signed payload plus the hash available before the HTTP submit begins.
#[derive(Clone)]
pub struct PreparedOrder {
    order_hash: String,
    payload: Value,
}

impl fmt::Debug for PreparedOrder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedOrder")
            .field("order_hash", &self.order_hash)
            .field("payload", &"[signed order redacted]")
            .finish()
    }
}

impl PreparedOrder {
    pub fn order_hash(&self) -> &str {
        &self.order_hash
    }

    pub(crate) fn payload(&self) -> &Value {
        &self.payload
    }

    /// Explicit access for callers that need to persist or inspect a signed order.
    pub fn expose_payload(&self) -> &Value {
        &self.payload
    }
}

#[derive(Debug, Clone)]
pub struct OrderBuilder {
    signer: AccountSigner,
}

impl OrderBuilder {
    pub fn new(signer: AccountSigner) -> Self {
        Self { signer }
    }

    pub fn signer(&self) -> &AccountSigner {
        &self.signer
    }

    pub fn prepare_limit_order(&self, params: &LimitOrderParams) -> Result<PreparedOrder> {
        if !params.post_only {
            return Err(Error::Config(
                "this LIMIT helper requires post_only=true".to_string(),
            ));
        }
        if params.expires_at_seconds == 0 {
            return Err(Error::Config(
                "expires_at_seconds must be non-zero".to_string(),
            ));
        }
        let token_id = U256::from_dec_str(params.token_id.trim()).map_err(|_| {
            Error::Config("token_id must be an unsigned decimal integer".to_string())
        })?;
        let amounts =
            limit_order_amounts(params.side, params.price_per_share_wei, params.quantity_wei)?;
        let maker = self.signer.order_address();
        let message = OrderMessage {
            salt: params.salt.unwrap_or_else(random_salt),
            maker,
            signer: maker,
            taker: parse_address(ZERO_ADDRESS)?,
            token_id,
            maker_amount: amounts.maker_amount,
            taker_amount: amounts.taker_amount,
            expiration: U256::from(params.expires_at_seconds),
            nonce: params.nonce,
            fee_rate_bps: U256::from(params.fee_rate_bps),
            side: params.side.as_u8(),
            signature_type: 0,
        };
        let hash = order_typed_data_hash(
            self.signer.chain_id,
            params.is_neg_risk,
            params.is_yield_bearing,
            &message,
        )?;
        let order_hash = h256_hex(hash);
        let signature = self.signer.sign_order_hash(hash)?;
        let signed = SignedOrder {
            hash: order_hash.clone(),
            salt: message.salt.to_string(),
            maker: address_hex(message.maker),
            signer: address_hex(message.signer),
            taker: address_hex(message.taker),
            token_id: message.token_id.to_string(),
            maker_amount: message.maker_amount.to_string(),
            taker_amount: message.taker_amount.to_string(),
            expiration: message.expiration.to_string(),
            nonce: message.nonce.to_string(),
            fee_rate_bps: message.fee_rate_bps.to_string(),
            side: message.side,
            signature_type: message.signature_type,
            signature,
        };
        Ok(PreparedOrder {
            order_hash,
            payload: json!({
                "data": {
                    "order": signed,
                    "pricePerShare": amounts.price_per_share.to_string(),
                    "strategy": "LIMIT",
                    "isPostOnly": true,
                    "isFillOrKill": false,
                }
            }),
        })
    }
}

#[derive(Debug, Clone)]
struct OrderAmounts {
    price_per_share: U256,
    maker_amount: U256,
    taker_amount: U256,
}

fn limit_order_amounts(side: OrderSide, price: U256, quantity: U256) -> Result<OrderAmounts> {
    let minimum_quantity = U256::exp10(16);
    if quantity < minimum_quantity {
        return Err(Error::Config(
            "quantity must be at least 0.01 shares".to_string(),
        ));
    }
    let precision = U256::exp10(18);
    let price = retain_significant_digits(price, 3);
    let quantity = retain_significant_digits(quantity, 5);
    if price.is_zero() || price >= precision {
        return Err(Error::Config(
            "price must be strictly between 0 and 1".to_string(),
        ));
    }
    Ok(match side {
        OrderSide::Buy => OrderAmounts {
            price_per_share: price,
            maker_amount: price * quantity / precision,
            taker_amount: quantity,
        },
        OrderSide::Sell => OrderAmounts {
            price_per_share: price,
            maker_amount: quantity,
            taker_amount: price * quantity / precision,
        },
    })
}

pub fn decimal_to_wei(value: &str) -> Result<U256> {
    let decimal = Decimal::from_str(value.trim())
        .map_err(|_| Error::Config(format!("invalid decimal value {value:?}")))?;
    if decimal.is_sign_negative() {
        return Err(Error::Config(
            "decimal value cannot be negative".to_string(),
        ));
    }
    if decimal.scale() > WEI_DECIMALS {
        return Err(Error::Config(
            "decimal value has more than 18 fractional digits".to_string(),
        ));
    }
    let mantissa = u128::try_from(decimal.mantissa())
        .map_err(|_| Error::Config("decimal value is too large".to_string()))?;
    Ok(U256::from(mantissa) * U256::exp10((WEI_DECIMALS - decimal.scale()) as usize))
}

#[derive(Debug, Clone)]
struct OrderMessage {
    salt: U256,
    maker: Address,
    signer: Address,
    taker: Address,
    token_id: U256,
    maker_amount: U256,
    taker_amount: U256,
    expiration: U256,
    nonce: U256,
    fee_rate_bps: U256,
    side: u8,
    signature_type: u8,
}

fn order_typed_data_hash(
    chain_id: u64,
    is_neg_risk: bool,
    is_yield_bearing: bool,
    order: &OrderMessage,
) -> Result<H256> {
    let verifying_contract = exchange_address(chain_id, is_neg_risk, is_yield_bearing)?;
    Ok(eip712_digest(
        eip712_domain_separator(
            PROTOCOL_NAME,
            PROTOCOL_VERSION,
            chain_id,
            verifying_contract,
        ),
        order_struct_hash(order),
    ))
}

fn order_struct_hash(order: &OrderMessage) -> H256 {
    let type_hash = keccak256(
        b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)",
    );
    H256::from(keccak256(encode(&[
        Token::FixedBytes(type_hash.to_vec()),
        Token::Uint(order.salt),
        Token::Address(order.maker),
        Token::Address(order.signer),
        Token::Address(order.taker),
        Token::Uint(order.token_id),
        Token::Uint(order.maker_amount),
        Token::Uint(order.taker_amount),
        Token::Uint(order.expiration),
        Token::Uint(order.nonce),
        Token::Uint(order.fee_rate_bps),
        Token::Uint(U256::from(order.side)),
        Token::Uint(U256::from(order.signature_type)),
    ])))
}

fn eip712_domain_separator(
    name: &str,
    version: &str,
    chain_id: u64,
    verifying_contract: Address,
) -> H256 {
    let type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    );
    H256::from(keccak256(encode(&[
        Token::FixedBytes(type_hash.to_vec()),
        Token::FixedBytes(keccak256(name.as_bytes()).to_vec()),
        Token::FixedBytes(keccak256(version.as_bytes()).to_vec()),
        Token::Uint(U256::from(chain_id)),
        Token::Address(verifying_contract),
    ])))
}

fn eip712_digest(domain_separator: H256, struct_hash: H256) -> H256 {
    let mut bytes = Vec::with_capacity(66);
    bytes.extend_from_slice(&[0x19, 0x01]);
    bytes.extend_from_slice(domain_separator.as_bytes());
    bytes.extend_from_slice(struct_hash.as_bytes());
    H256::from(keccak256(bytes))
}

fn hash_kernel_message(message_hash: H256) -> H256 {
    let type_hash = keccak256(b"Kernel(bytes32 hash)");
    H256::from(keccak256(encode(&[
        Token::FixedBytes(type_hash.to_vec()),
        Token::FixedBytes(message_hash.as_bytes().to_vec()),
    ])))
}

fn eip191_text_hash(message: &str) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut bytes = Vec::with_capacity(prefix.len() + message.len());
    bytes.extend_from_slice(prefix.as_bytes());
    bytes.extend_from_slice(message.as_bytes());
    keccak256(bytes)
}

fn eip191_bytes_hash(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut bytes = Vec::with_capacity(prefix.len() + message.len());
    bytes.extend_from_slice(prefix.as_bytes());
    bytes.extend_from_slice(message);
    keccak256(bytes)
}

fn exchange_address(chain_id: u64, is_neg_risk: bool, is_yield_bearing: bool) -> Result<Address> {
    ensure_supported_chain(chain_id)?;
    parse_address(match (is_neg_risk, is_yield_bearing) {
        (false, false) => CTF_EXCHANGE_MAINNET,
        (true, false) => NEG_RISK_CTF_EXCHANGE_MAINNET,
        (false, true) => YIELD_BEARING_CTF_EXCHANGE_MAINNET,
        (true, true) => YIELD_BEARING_NEG_RISK_CTF_EXCHANGE_MAINNET,
    })
}

fn ensure_supported_chain(chain_id: u64) -> Result<()> {
    if chain_id == 56 {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "unsupported chain id {chain_id}; v0.1 supports BNB mainnet only"
        )))
    }
}

fn parse_wallet(private_key: &str) -> Result<LocalWallet> {
    LocalWallet::from_str(private_key)
        .map_err(|_| Error::Config("private key is not a valid EVM key".to_string()))
}

fn parse_address(value: &str) -> Result<Address> {
    Address::from_str(value).map_err(|_| Error::Config("invalid EVM address".to_string()))
}

fn random_salt() -> U256 {
    U256::from(rand::thread_rng().gen_range(0..=MAX_SALT))
}

fn retain_significant_digits(value: U256, significant_digits: usize) -> U256 {
    if value.is_zero() {
        return value;
    }
    let magnitude = value.to_string().len();
    if magnitude <= significant_digits {
        return value;
    }
    let divisor = U256::exp10(magnitude - significant_digits);
    value / divisor * divisor
}

fn h256_hex(value: H256) -> String {
    format!("0x{}", hex::encode(value.as_bytes()))
}

fn address_hex(value: Address) -> String {
    format!("0x{}", hex::encode(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_private_key() -> String {
        format!("0x{:064x}", 1u8)
    }

    fn params() -> LimitOrderParams {
        let mut params = LimitOrderParams::post_only_buy(
            "12345678901234567890",
            0,
            false,
            false,
            decimal_to_wei("0.27").unwrap(),
            decimal_to_wei("150").unwrap(),
            4_102_444_800,
        );
        params.salt = Some(U256::from(7));
        params
    }

    #[test]
    fn decimal_conversion_is_exact() {
        assert_eq!(decimal_to_wei("0.01").unwrap(), U256::exp10(16));
        assert_eq!(
            decimal_to_wei("230").unwrap(),
            U256::from(230) * U256::exp10(18)
        );
        assert!(decimal_to_wei("-1").is_err());
    }

    #[test]
    fn signed_vector_matches_existing_native_maker() {
        let builder =
            OrderBuilder::new(AccountSigner::bnb_mainnet_eoa(&synthetic_private_key()).unwrap());
        let prepared = builder.prepare_limit_order(&params()).unwrap();
        assert_eq!(
            prepared.order_hash(),
            "0xa7731e4893ceb56f003e4ed0527d8665c35f876d853514c7c6c0bc24676c94f2"
        );
        assert_eq!(prepared.payload()["data"]["isPostOnly"], true);
        assert_eq!(prepared.payload()["data"]["isFillOrKill"], false);
        assert_eq!(prepared.payload()["data"]["order"]["side"], 0);
        assert_eq!(
            prepared.payload()["data"]["order"]["signature"],
            "eda10b7a8b4fa8aa6f29b88551945355af8a41cb93b6cb4393efb8ca5697b4416544ad548b78f9dda06e543a83fd900aa78e4be5d9b8bd8cb9a162fcf61af87b1b"
        );
    }

    #[test]
    fn all_exchange_domains_are_distinct() {
        let addresses = [
            exchange_address(56, false, false).unwrap(),
            exchange_address(56, true, false).unwrap(),
            exchange_address(56, false, true).unwrap(),
            exchange_address(56, true, true).unwrap(),
        ];
        for (index, address) in addresses.iter().enumerate() {
            assert!(!address.is_zero());
            assert!(!addresses[..index].contains(address));
        }
    }

    #[test]
    fn predict_account_signature_has_kernel_prefix() {
        let signer = AccountSigner::bnb_mainnet_predict_account(
            &synthetic_private_key(),
            "0x000000000000000000000000000000000000dEaD",
        )
        .unwrap();
        let signature = signer.sign_predict_account_hash(H256::zero()).unwrap();
        assert!(signature.starts_with("0x01"));
        assert_eq!(signature.len(), 2 + 2 + 40 + 130);
    }

    #[test]
    fn prepared_order_debug_redacts_signed_payload() {
        let builder =
            OrderBuilder::new(AccountSigner::bnb_mainnet_eoa(&synthetic_private_key()).unwrap());
        let prepared = builder.prepare_limit_order(&params()).unwrap();
        let debug = format!("{prepared:?}");
        assert!(debug.contains("signed order redacted"));
        assert!(!debug.contains("signature"));
    }
}
