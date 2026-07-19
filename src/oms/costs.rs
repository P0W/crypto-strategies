use crate::config::{ExchangeConfig, FixedChargeFrequency, TransactionCostConfig};
use crate::oms::types::OrderId;
use crate::{Money, Side, Symbol};
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub struct TransactionCostCalculator {
    model: TransactionCostModel,
}

enum TransactionCostModel {
    Percentage { maker_rate: f64, taker_rate: f64 },
    Components(ComponentCosts),
}

struct ComponentCosts {
    brokerage_rate: f64,
    brokerage_cap_per_order: f64,
    buy_turnover_rate: f64,
    sell_turnover_rate: f64,
    exchange_rate: f64,
    regulatory_rate: f64,
    buy_stamp_rate: f64,
    indirect_tax_rate: f64,
    sell_fixed_charge: f64,
    sell_fixed_charge_frequency: FixedChargeFrequency,
    brokerage_charged: Mutex<HashMap<(OrderId, NaiveDate), f64>>,
    charged_sell_fixed: Mutex<HashSet<(Symbol, NaiveDate)>>,
}

impl TransactionCostCalculator {
    pub fn from_exchange_config(config: &ExchangeConfig) -> Self {
        let model = match &config.cost_model {
            TransactionCostConfig::Percentage => TransactionCostModel::Percentage {
                maker_rate: config.maker_fee,
                taker_rate: config.taker_fee,
            },
            TransactionCostConfig::Components {
                brokerage_rate,
                brokerage_cap_per_order,
                buy_turnover_rate,
                sell_turnover_rate,
                exchange_rate,
                regulatory_rate,
                buy_stamp_rate,
                indirect_tax_rate,
                sell_fixed_charge,
                sell_fixed_charge_frequency,
            } => TransactionCostModel::Components(ComponentCosts {
                brokerage_rate: *brokerage_rate,
                brokerage_cap_per_order: *brokerage_cap_per_order,
                buy_turnover_rate: *buy_turnover_rate,
                sell_turnover_rate: *sell_turnover_rate,
                exchange_rate: *exchange_rate,
                regulatory_rate: *regulatory_rate,
                buy_stamp_rate: *buy_stamp_rate,
                indirect_tax_rate: *indirect_tax_rate,
                sell_fixed_charge: *sell_fixed_charge,
                sell_fixed_charge_frequency: *sell_fixed_charge_frequency,
                brokerage_charged: Mutex::new(HashMap::new()),
                charged_sell_fixed: Mutex::new(HashSet::new()),
            }),
        };
        Self { model }
    }

    pub fn percentage(maker_rate: f64, taker_rate: f64) -> Self {
        Self {
            model: TransactionCostModel::Percentage {
                maker_rate,
                taker_rate,
            },
        }
    }

    pub fn calculate(
        &self,
        order_id: OrderId,
        symbol: &Symbol,
        side: Side,
        turnover: f64,
        is_maker: bool,
        timestamp: DateTime<Utc>,
    ) -> Money {
        let cost = match &self.model {
            TransactionCostModel::Percentage {
                maker_rate,
                taker_rate,
            } => turnover * if is_maker { maker_rate } else { taker_rate },
            TransactionCostModel::Components(costs) => {
                costs.calculate(order_id, symbol, side, turnover, timestamp.date_naive())
            }
        };
        Money::from_f64(cost)
    }

    pub fn estimate(&self, side: Side, turnover: f64, is_maker: bool) -> Money {
        let cost = match &self.model {
            TransactionCostModel::Percentage {
                maker_rate,
                taker_rate,
            } => turnover * if is_maker { maker_rate } else { taker_rate },
            TransactionCostModel::Components(costs) => costs.estimate(side, turnover),
        };
        Money::from_f64(cost)
    }
}

impl ComponentCosts {
    fn calculate(
        &self,
        order_id: OrderId,
        symbol: &Symbol,
        side: Side,
        turnover: f64,
        trade_date: NaiveDate,
    ) -> f64 {
        let brokerage = self.brokerage_for_fill(order_id, turnover, trade_date);
        let fixed = self.sell_fixed_charge(symbol, side, trade_date);
        self.total(side, turnover, brokerage, fixed)
    }

    fn estimate(&self, side: Side, turnover: f64) -> f64 {
        let uncapped_brokerage = turnover * self.brokerage_rate;
        let brokerage = if self.brokerage_cap_per_order > 0.0 {
            uncapped_brokerage.min(self.brokerage_cap_per_order)
        } else {
            uncapped_brokerage
        };
        let fixed = if side == Side::Sell {
            self.sell_fixed_charge
        } else {
            0.0
        };
        self.total(side, turnover, brokerage, fixed)
    }

    fn total(&self, side: Side, turnover: f64, brokerage: f64, fixed: f64) -> f64 {
        let turnover_charge = turnover
            * match side {
                Side::Buy => self.buy_turnover_rate,
                Side::Sell => self.sell_turnover_rate,
            };
        let exchange = turnover * self.exchange_rate;
        let regulatory = turnover * self.regulatory_rate;
        let stamp = if side == Side::Buy {
            turnover * self.buy_stamp_rate
        } else {
            0.0
        };
        let indirect_tax = (brokerage + exchange + regulatory) * self.indirect_tax_rate;
        brokerage + turnover_charge + exchange + regulatory + stamp + indirect_tax + fixed
    }

    fn brokerage_for_fill(&self, order_id: OrderId, turnover: f64, trade_date: NaiveDate) -> f64 {
        let uncapped = turnover * self.brokerage_rate;
        if self.brokerage_cap_per_order <= 0.0 {
            return uncapped;
        }

        let mut charged = self
            .brokerage_charged
            .lock()
            .expect("brokerage state poisoned");
        charged.retain(|(_, date), _| *date >= trade_date);
        let key = (order_id, trade_date);
        let previous = charged.get(&key).copied().unwrap_or(0.0);
        let current = uncapped.min((self.brokerage_cap_per_order - previous).max(0.0));
        charged.insert(key, previous + current);
        current
    }

    fn sell_fixed_charge(&self, symbol: &Symbol, side: Side, trade_date: NaiveDate) -> f64 {
        if side != Side::Sell || self.sell_fixed_charge <= 0.0 {
            return 0.0;
        }

        match self.sell_fixed_charge_frequency {
            FixedChargeFrequency::PerFill => self.sell_fixed_charge,
            FixedChargeFrequency::PerSymbolPerDay => {
                let mut charged = self
                    .charged_sell_fixed
                    .lock()
                    .expect("fixed charge state poisoned");
                charged.retain(|(_, date)| *date >= trade_date);
                if charged.insert((symbol.clone(), trade_date)) {
                    self.sell_fixed_charge
                } else {
                    0.0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn component_config(
        brokerage_rate: f64,
        brokerage_cap_per_order: f64,
        sell_fixed_charge: f64,
        frequency: FixedChargeFrequency,
    ) -> ExchangeConfig {
        ExchangeConfig {
            cost_model: TransactionCostConfig::Components {
                brokerage_rate,
                brokerage_cap_per_order,
                buy_turnover_rate: 0.001,
                sell_turnover_rate: 0.001,
                exchange_rate: 0.0000307,
                regulatory_rate: 0.000001,
                buy_stamp_rate: 0.00015,
                indirect_tax_rate: 0.18,
                sell_fixed_charge,
                sell_fixed_charge_frequency: frequency,
            },
            ..ExchangeConfig::default()
        }
    }

    #[test]
    fn component_costs_are_asymmetric_and_fixed_charge_is_daily() {
        let calculator = TransactionCostCalculator::from_exchange_config(&component_config(
            0.0,
            0.0,
            15.34,
            FixedChargeFrequency::PerSymbolPerDay,
        ));
        let symbol = Symbol::new("NIFTYBEES");
        let timestamp = Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0).unwrap();

        let buy = calculator
            .calculate(1, &symbol, Side::Buy, 100_000.0, false, timestamp)
            .to_f64();
        let sell = calculator
            .calculate(2, &symbol, Side::Sell, 100_000.0, false, timestamp)
            .to_f64();
        let second_sell = calculator
            .calculate(3, &symbol, Side::Sell, 50_000.0, false, timestamp)
            .to_f64();

        assert!((buy - 118.7406).abs() < 1e-5);
        assert!((sell - 119.0806).abs() < 1e-5);
        assert!((sell - second_sell - 67.2103).abs() < 1e-5);
    }

    #[test]
    fn brokerage_cap_is_shared_across_partial_fills() {
        let calculator = TransactionCostCalculator::from_exchange_config(&component_config(
            0.0003,
            20.0,
            0.0,
            FixedChargeFrequency::PerFill,
        ));
        let timestamp = Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0).unwrap();
        let first = calculator
            .calculate(
                1,
                &Symbol::new("RELIANCE"),
                Side::Buy,
                1_000_000.0,
                false,
                timestamp,
            )
            .to_f64();
        let second = calculator
            .calculate(
                1,
                &Symbol::new("RELIANCE"),
                Side::Buy,
                1_000_000.0,
                false,
                timestamp,
            )
            .to_f64();
        let next_day = calculator
            .calculate(
                1,
                &Symbol::new("RELIANCE"),
                Side::Buy,
                1_000_000.0,
                false,
                timestamp + chrono::Duration::days(1),
            )
            .to_f64();

        assert!((first - second - 23.6).abs() < 1e-5);
        assert!((next_day - first).abs() < 1e-5);
    }

    #[test]
    fn percentage_model_preserves_crypto_behavior() {
        let calculator = TransactionCostCalculator::percentage(0.001, 0.002);
        let timestamp = Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0).unwrap();

        assert_eq!(
            calculator
                .calculate(
                    1,
                    &Symbol::new("BTCINR"),
                    Side::Buy,
                    10_000.0,
                    true,
                    timestamp,
                )
                .to_f64(),
            10.0
        );
        assert_eq!(
            calculator
                .calculate(
                    2,
                    &Symbol::new("BTCINR"),
                    Side::Buy,
                    10_000.0,
                    false,
                    timestamp,
                )
                .to_f64(),
            20.0
        );
    }
}
