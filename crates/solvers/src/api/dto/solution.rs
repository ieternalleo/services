use crate::util::serialize;
use ethereum_types::{H160, U256};
use serde::Serialize;
use serde_with::serde_as;
use std::collections::HashMap;

impl Solution {
    /// Returns the trivial solution.
    pub fn trivial() -> Self {
        Self {
            prices: Default::default(),
            trades: Default::default(),
            interactions: Default::default(),
        }
    }
}

#[serde_as]
#[derive(Debug, Serialize)]
pub struct Solution {
    #[serde_as(as = "HashMap<_, serialize::U256>")]
    prices: HashMap<H160, U256>,
    trades: Vec<Trade>,
    interactions: Vec<Interaction>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Trade {
    Fulfillment(Fulfillment),
    Jit(JitTrade),
}

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Fulfillment {
    #[serde_as(as = "serialize::Hex")]
    order: [u8; 56],
    #[serde_as(as = "serialize::U256")]
    executed_amount: U256,
}

#[serde_as]
#[derive(Debug, Serialize)]
struct JitTrade {
    order: JitOrder,
    #[serde_as(as = "serialize::U256")]
    executed_amount: U256,
}

#[serde_as]
#[derive(Debug, Serialize)]
struct JitOrder {
    sell_token: H160,
    buy_token: H160,
    receiver: H160,
    #[serde_as(as = "serialize::U256")]
    sell_amount: U256,
    #[serde_as(as = "serialize::U256")]
    buy_amount: U256,
    valid_to: u32,
    #[serde_as(as = "serialize::Hex")]
    app_data: [u8; 32],
    #[serde_as(as = "serialize::U256")]
    fee_amount: U256,
    kind: Kind,
    partially_fillable: bool,
    sell_token_balance: SellTokenBalance,
    buy_token_balance: BuyTokenBalance,
    signing_scheme: SigningScheme,
    #[serde_as(as = "serialize::Hex")]
    signature: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Sell,
    Buy,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
enum Interaction {
    Liquidity(LiquidityInteraction),
    Custom(CustomInteraction),
}

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiquidityInteraction {
    internalize: bool,
    id: usize,
    input_token: H160,
    output_token: H160,
    #[serde_as(as = "serialize::U256")]
    input_amount: U256,
    #[serde_as(as = "serialize::U256")]
    output_amount: U256,
}

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomInteraction {
    internalize: bool,
    target: H160,
    #[serde_as(as = "serialize::U256")]
    value: U256,
    #[serde_as(as = "serialize::Hex")]
    call_data: Vec<u8>,
    allowances: Vec<Allowance>,
    inputs: Vec<Asset>,
    outputs: Vec<Asset>,
}

#[serde_as]
#[derive(Debug, Serialize)]
struct Asset {
    token: H160,
    #[serde_as(as = "serialize::U256")]
    amount: U256,
}

#[serde_as]
#[derive(Debug, Serialize)]
struct Allowance {
    token: H160,
    spender: H160,
    #[serde_as(as = "serialize::U256")]
    amount: U256,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
enum SellTokenBalance {
    #[default]
    Erc20,
    Internal,
    External,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
enum BuyTokenBalance {
    #[default]
    Erc20,
    Internal,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum SigningScheme {
    Eip712,
    EthSign,
    PreSign,
    Eip1271,
}
