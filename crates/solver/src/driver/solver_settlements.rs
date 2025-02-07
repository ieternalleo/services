use crate::{settlement::Settlement, solver::Solver};
use ethcontract::U256;
use model::auction::AuctionId;
use num::BigRational;
use shared::{
    conversions::U256Ext as _,
    http_solver::model::{AuctionResult, SolverRejectionReason},
};
use std::{collections::HashSet, sync::Arc, time::Duration};

pub fn has_user_order(settlement: &Settlement) -> bool {
    settlement.user_trades().next().is_some()
}

// Each individual settlement has an objective value.
#[derive(Debug, Clone)]
pub struct RatedSettlement {
    // Identifies a settlement during a run loop.
    pub id: usize,
    pub settlement: Settlement,
    pub surplus: BigRational,                 // In wei.
    pub unscaled_subsidized_fee: BigRational, // In wei.
    pub scaled_unsubsidized_fee: BigRational, // In wei.
    pub gas_estimate: U256,                   // In gas units.
    pub gas_price: BigRational,               // In wei per gas unit.
}

// Helper function for RatedSettlement to allow unit testing objective value computation
// without a Settlement.
fn compute_objective_value(
    surplus: &BigRational,
    solver_fees: &BigRational,
    gas_estimate: &BigRational,
    gas_price: &BigRational,
) -> BigRational {
    let cost = gas_estimate * gas_price;
    surplus + solver_fees - cost
}

impl RatedSettlement {
    pub fn objective_value(&self) -> BigRational {
        let gas_estimate = self.gas_estimate.to_big_rational();
        compute_objective_value(
            &self.surplus,
            &self.scaled_unsubsidized_fee,
            &gas_estimate,
            &self.gas_price,
        )
    }
}

/// Filters out all settlements without any user order which is mature by age or mature by association.
/// Any user order older than `min_order_age` is considered to be mature by age.
/// Any younger user order in a settlement containing a user order mature by age or mature by association
/// is considered to be mature by association.
/// Old liquidity orders can not contribute to the maturity of a settlement.
/// Because maturity by association is defined recursively it can "spread" across settlements,
/// resulting in settlements being allowed where it's not immediately obvious by which association
/// any user order of a settlement has matured.
pub fn retain_mature_settlements(
    min_order_age: Duration,
    settlements: Vec<(Arc<dyn Solver>, Settlement)>,
    auction_id: AuctionId,
) -> Vec<(Arc<dyn Solver>, Settlement)> {
    fn find_mature_settlements(
        min_order_age: Duration,
        settlements: &[(Arc<dyn Solver>, Settlement)],
    ) -> HashSet<usize> {
        let settle_orders_older_than =
            chrono::offset::Utc::now() - chrono::Duration::from_std(min_order_age).unwrap();

        let mut valid_trades = HashSet::<&model::order::OrderUid>::default();
        let mut valid_settlement_indices = HashSet::<usize>::default();

        loop {
            let mut new_order_added = false;

            for (index, (_, settlement)) in settlements.iter().enumerate() {
                if valid_settlement_indices.contains(&index) {
                    continue;
                }
                let contains_valid_user_trade = settlement.user_trades().any(|trade| {
                    // mature by age
                    trade.order.metadata.creation_date <= settle_orders_older_than
                    // mature by association
                    || valid_trades.contains(&trade.order.metadata.uid)
                });

                if contains_valid_user_trade {
                    for trade in settlement.user_trades() {
                        // make all user orders within this settlement mature by association
                        new_order_added |= valid_trades.insert(&trade.order.metadata.uid);
                    }
                    valid_settlement_indices.insert(index);
                }
            }

            if !new_order_added {
                break valid_settlement_indices;
            }
        }
    }

    let valid_settlement_indices = find_mature_settlements(min_order_age, &settlements);

    for (_, (solver, settlement)) in settlements
        .iter()
        .enumerate()
        .filter(|(i, _)| !valid_settlement_indices.contains(i))
    {
        tracing::debug!(
            solver_name = %solver.name(), ?settlement,
            "filtered settlement for not including any mature orders",
        );
        solver.notify_auction_result(
            auction_id,
            AuctionResult::Rejected(SolverRejectionReason::NoMatureOrders),
        );
    }

    settlements
        .into_iter()
        .enumerate()
        .filter(|(index, _)| valid_settlement_indices.contains(index))
        .map(|(_, item)| item)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settlement::Trade, solver::dummy_arc_solver};
    use chrono::{offset::Utc, DateTime, Duration, Local};

    use model::order::{LimitOrderClass, Order, OrderClass, OrderData, OrderMetadata, OrderUid};
    use num::BigRational;

    use std::ops::Sub;

    fn trade(created_at: DateTime<Utc>, uid: u8, class: OrderClass) -> Trade {
        Trade {
            order: Order {
                data: OrderData {
                    sell_amount: 1.into(),
                    buy_amount: 1.into(),
                    ..Default::default()
                },
                metadata: OrderMetadata {
                    creation_date: created_at,
                    uid: OrderUid([uid; 56]),
                    class,
                    ..Default::default()
                },
                ..Default::default()
            },
            executed_amount: 1.into(),
            ..Default::default()
        }
    }

    fn settlements_into_dummy_solver_settlements(
        settlements: Vec<Settlement>,
    ) -> Vec<(Arc<dyn Solver>, Settlement)> {
        settlements
            .into_iter()
            .map(|settlement| (dummy_arc_solver(), settlement))
            .collect()
    }

    fn solver_settlements_into_settlements(
        solver_settlements: &[(Arc<dyn Solver>, Settlement)],
    ) -> Vec<Settlement> {
        solver_settlements
            .iter()
            .map(|(_, settlement)| settlement.clone())
            .collect()
    }

    fn assert_same_settlements(expected: &[Settlement], actual: &[Settlement]) {
        assert!(expected
            .iter()
            .map(|s| s.trades().collect::<Vec<_>>())
            .eq(actual.iter().map(|s| s.trades().collect::<Vec<_>>())));
    }

    #[test]
    fn no_mature_orders() {
        let recent = Local::now().with_timezone(&Utc);
        let min_age = std::time::Duration::from_secs(50);

        let s1 = Settlement::with_default_prices(vec![
            trade(recent, 1, OrderClass::Market),
            trade(recent, 2, OrderClass::Market),
        ]);
        let s2 = Settlement::with_default_prices(vec![
            trade(recent, 2, OrderClass::Market),
            trade(recent, 3, OrderClass::Market),
        ]);
        let s3 = Settlement::with_default_prices(vec![
            trade(recent, 4, OrderClass::Market),
            trade(recent, 5, OrderClass::Market),
        ]);
        let settlements = vec![s1, s2, s3];
        let mature_settlements = retain_mature_settlements(
            min_age,
            settlements_into_dummy_solver_settlements(settlements),
            0,
        );

        assert_same_settlements(
            &solver_settlements_into_settlements(&mature_settlements),
            &[],
        );
    }

    #[test]
    fn mature_by_age() {
        let recent = Local::now().with_timezone(&Utc);
        let old = Local::now().with_timezone(&Utc).sub(Duration::seconds(600));
        let min_age = std::time::Duration::from_secs(60);

        let s1 = Settlement::with_default_prices(vec![
            trade(old, 1, OrderClass::Market),
            trade(
                recent,
                2,
                OrderClass::Limit(LimitOrderClass {
                    surplus_fee: Some(Default::default()),
                    surplus_fee_timestamp: Some(Default::default()),
                    executed_surplus_fee: None,
                }),
            ),
        ]);
        let s2 = Settlement::with_default_prices(vec![
            trade(recent, 3, OrderClass::Market),
            trade(recent, 4, OrderClass::Market),
        ]);
        let s3 = Settlement::with_default_prices(vec![
            trade(recent, 5, OrderClass::Market),
            trade(old, 6, OrderClass::Liquidity),
        ]);
        let settlements = vec![s1.clone(), s2, s3];
        let mature_settlements = retain_mature_settlements(
            min_age,
            settlements_into_dummy_solver_settlements(settlements),
            0,
        );

        assert_same_settlements(
            &solver_settlements_into_settlements(&mature_settlements),
            &[s1],
        );
    }

    #[test]
    fn mature_by_association() {
        let recent = Local::now().with_timezone(&Utc);
        let old = Local::now().with_timezone(&Utc).sub(Duration::seconds(600));
        let min_age = std::time::Duration::from_secs(60);

        let s1 = Settlement::with_default_prices(vec![
            trade(recent, 1, OrderClass::Market),
            trade(recent, 2, OrderClass::Market),
        ]);
        let s2 = Settlement::with_default_prices(vec![
            trade(recent, 2, OrderClass::Market),
            trade(recent, 3, OrderClass::Market),
        ]);
        let s3 = Settlement::with_default_prices(vec![
            trade(recent, 3, OrderClass::Market),
            trade(old, 4, OrderClass::Market),
        ]);
        // this will not be included because it only contains recent orders which are not
        // referenced in any other valid settlements
        let s4 = Settlement::with_default_prices(vec![
            trade(recent, 5, OrderClass::Market),
            trade(recent, 6, OrderClass::Market),
        ]);
        let settlements = vec![s1.clone(), s2.clone(), s3.clone(), s4];
        let mature_settlements = retain_mature_settlements(
            min_age,
            settlements_into_dummy_solver_settlements(settlements),
            0,
        );

        assert_same_settlements(
            &solver_settlements_into_settlements(&mature_settlements),
            &[s1, s2, s3],
        );
    }

    #[test]
    fn mature_by_association_in_between() {
        let recent = Local::now().with_timezone(&Utc);
        let old = Local::now().with_timezone(&Utc).sub(Duration::seconds(600));
        let min_age = std::time::Duration::from_secs(60);

        let s1 = Settlement::with_default_prices(vec![
            trade(old, 1, OrderClass::Market),
            trade(recent, 2, OrderClass::Market),
        ]);
        let s2 = Settlement::with_default_prices(vec![trade(recent, 3, OrderClass::Market)]);
        let s3 = Settlement::with_default_prices(vec![
            trade(recent, 2, OrderClass::Market),
            trade(recent, 3, OrderClass::Market),
        ]);
        let s4 = Settlement::with_default_prices(vec![trade(recent, 3, OrderClass::Market)]);
        let settlements = vec![s1.clone(), s2.clone(), s3.clone(), s4.clone()];
        let mature_settlements = retain_mature_settlements(
            min_age,
            settlements_into_dummy_solver_settlements(settlements),
            0,
        );

        assert_same_settlements(
            &solver_settlements_into_settlements(&mature_settlements),
            &[s1, s2, s3, s4],
        );
    }

    #[test]
    fn mature_by_association_of_liquidity_order_is_not_accepted() {
        let recent = Local::now().with_timezone(&Utc);
        let old = Local::now().with_timezone(&Utc).sub(Duration::seconds(600));
        let min_age = std::time::Duration::from_secs(60);

        let s1 = Settlement::with_default_prices(vec![
            trade(recent, 1, OrderClass::Market),
            trade(recent, 2, OrderClass::Market),
        ]);
        let s2 = Settlement::with_default_prices(vec![
            trade(recent, 2, OrderClass::Market),
            trade(old, 3, OrderClass::Liquidity),
        ]);
        let settlements = vec![s1, s2];
        let mature_settlements = retain_mature_settlements(
            min_age,
            settlements_into_dummy_solver_settlements(settlements),
            0,
        );
        assert_same_settlements(
            &solver_settlements_into_settlements(&mature_settlements),
            &[],
        );
    }

    #[test]
    fn compute_objective_value() {
        // Surplus1 is 1.003 ETH
        let surplus1 = BigRational::from_integer(1_003_000_000_000_000_000_u128.into());

        // Surplus2 is 1.009 ETH
        let surplus2 = BigRational::from_integer(1_009_000_000_000_000_000_u128.into());

        // Fees is 0.001 ETH
        let solver_fees = BigRational::from_integer(1_000_000_000_000_000_u128.into());

        let gas_estimate1 = BigRational::from_integer(300_000.into());
        let gas_estimate2 = BigRational::from_integer(500_000.into());

        // Three cases when using three different gas prices:

        // Case 1: objective value 1 < objective value 2

        // Gas price is 10 gwei
        let gas_price = BigRational::from_integer(10_000_000_000_u128.into());

        // Objective value 1 is 1.004 - 3e5 * 10e-9 = 1.001 ETH
        let obj_value1 =
            super::compute_objective_value(&surplus1, &solver_fees, &gas_estimate1, &gas_price);

        assert_eq!(
            obj_value1,
            BigRational::from_integer(1_001_000_000_000_000_000_u128.into())
        );

        // Objective value 2 is 1.01 - 5e5 * 10e-9 = 1.005 ETH
        let obj_value2 =
            super::compute_objective_value(&surplus2, &solver_fees, &gas_estimate2, &gas_price);

        assert_eq!(
            obj_value2,
            BigRational::from_integer(1_005_000_000_000_000_000_u128.into())
        );

        assert!(obj_value1 < obj_value2);

        // Case 2: objective value 1 = objective value 2

        // Gas price is 30 gwei
        let gas_price = BigRational::from_integer(30_000_000_000_u128.into());

        // Objective value 1 is 1.004 - 3e5 * 30e-9 = 0.995 ETH
        let obj_value1 =
            super::compute_objective_value(&surplus1, &solver_fees, &gas_estimate1, &gas_price);

        assert_eq!(
            obj_value1,
            BigRational::from_integer(995_000_000_000_000_000_u128.into())
        );

        // Objective value 2 is 1.01 - 5e5 * 30e-9 = 0.995 ETH
        let obj_value2 =
            super::compute_objective_value(&surplus2, &solver_fees, &gas_estimate2, &gas_price);

        assert_eq!(
            obj_value2,
            BigRational::from_integer(995_000_000_000_000_000_u128.into())
        );

        assert!(obj_value1 == obj_value2);

        // Case 3: objective value 1 > objective value 2

        // Gas price is 50 gwei
        let gas_price = BigRational::from_integer(50_000_000_000_u128.into());

        // Objective value 1 is 1.004 - 3e5 * 50e-9 = 0.989 ETH
        let obj_value1 =
            super::compute_objective_value(&surplus1, &solver_fees, &gas_estimate1, &gas_price);

        assert_eq!(
            obj_value1,
            BigRational::from_integer(989_000_000_000_000_000_u128.into())
        );

        // Objective value 2 is 1.01 - 5e5 * 50e-9 = 0.985 ETH
        let obj_value2 =
            super::compute_objective_value(&surplus2, &solver_fees, &gas_estimate2, &gas_price);

        assert_eq!(
            obj_value2,
            BigRational::from_integer(985_000_000_000_000_000_u128.into())
        );

        assert!(obj_value1 > obj_value2);
    }

    #[test]
    fn has_user_order_() {
        let order = |class| trade(Default::default(), 0, class);

        let settlement = Settlement::with_default_prices(vec![]);
        assert!(!has_user_order(&settlement));

        let settlement =
            Settlement::with_default_prices(vec![order(OrderClass::Limit(LimitOrderClass {
                surplus_fee: Some(Default::default()),
                surplus_fee_timestamp: Some(Default::default()),
                executed_surplus_fee: None,
            }))]);
        assert!(has_user_order(&settlement));

        let settlement = Settlement::with_default_prices(vec![order(OrderClass::Liquidity)]);
        assert!(!has_user_order(&settlement));

        let settlement = Settlement::with_default_prices(vec![order(OrderClass::Market)]);
        assert!(has_user_order(&settlement));

        let settlement = Settlement::with_default_prices(vec![
            order(OrderClass::Market),
            order(OrderClass::Liquidity),
        ]);
        assert!(has_user_order(&settlement));

        let settlement = Settlement::with_default_prices(vec![
            order(OrderClass::Liquidity),
            order(OrderClass::Limit(LimitOrderClass {
                surplus_fee: Some(Default::default()),
                surplus_fee_timestamp: Some(Default::default()),
                executed_surplus_fee: None,
            })),
        ]);
        assert!(has_user_order(&settlement));
    }
}
