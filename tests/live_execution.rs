use anyhow::{bail, ensure, Result};
use chrono::{Duration, Utc};
use crypto_strategies::oms::broker::{
    Balance, Broker, BrokerOrder, BrokerReport, BrokerStatus, MarketRules, Quote,
};
use crypto_strategies::oms::live_execution::LiveExecution;
use crypto_strategies::oms::{OrderType, TradingEngine};
use crypto_strategies::state_manager::SqliteStateManager;
use crypto_strategies::{
    Candle, Config, Money, MultiSymbolMultiTimeframeData, MultiTimeframeData, OrderRequest,
    Position, Side, Strategy, StrategyContext, Symbol,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn m(value: f64) -> Money {
    Money::from_f64(value)
}

fn config() -> Config {
    serde_json::from_value(serde_json::json!({
        "exchange": {"maker_fee": 0.001, "taker_fee": 0.001, "assumed_slippage": 0.001, "rate_limit": 10},
        "trading": {
            "symbols": ["TESTUSD"], "initial_capital": 1000.0, "risk_per_trade": 0.1,
            "max_positions": 1, "max_portfolio_heat": 0.5, "max_position_pct": 0.9,
            "max_drawdown": 0.5, "drawdown_warning": 0.3, "drawdown_critical": 0.4,
            "drawdown_warning_multiplier": 1.0, "drawdown_critical_multiplier": 1.0,
            "consecutive_loss_limit": 3, "consecutive_loss_multiplier": 0.5
        },
        "strategy": {"name": "probe", "timeframe": "1h"},
        "tax": {"tax_rate": 0.0, "tds_rate": 0.0, "loss_offset_allowed": true},
        "backtest": {"data_dir": "data", "results_dir": "results", "commission": 0.0, "use_t1_execution": false}
    })).unwrap()
}

#[derive(Clone)]
struct Probe;

impl Strategy for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(self.clone())
    }
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        if ctx.current_position.is_none() && ctx.open_orders.is_empty() {
            vec![OrderRequest::market_buy(ctx.symbol.clone(), 2.0).with_quantity_cap()]
        } else {
            vec![]
        }
    }
    fn calculate_stop_loss(&self, _: &[Candle], _: f64, _: Side) -> f64 {
        90.0
    }
    fn calculate_take_profit(&self, _: &[Candle], _: f64, _: Side) -> f64 {
        130.0
    }
    fn update_trailing_stop(&self, _: &Position, _: f64, _: &[Candle]) -> Option<f64> {
        None
    }
}

fn frame() -> MultiSymbolMultiTimeframeData {
    let time = Utc::now() - Duration::hours(1);
    let mut mtf = MultiTimeframeData::new("1h");
    mtf.add_timeframe(
        "1h",
        vec![Candle::new(time, 100.0, 101.0, 99.0, 100.0, 1000.0).unwrap()],
    );
    MultiSymbolMultiTimeframeData::from([(Symbol::new("TESTUSD"), mtf)])
}

fn rules() -> MarketRules {
    MarketRules {
        symbol: Symbol::new("TESTUSD"),
        asset: "TEST".into(),
        quote: "USD".into(),
        data_pair: "test-feed".into(),
        quantity_step: m(0.01),
        price_tick: m(0.01),
        min_quantity: m(0.01),
        max_quantity: m(1000.0),
        max_market_quantity: m(1000.0),
        min_notional: m(1.0),
        min_price: m(0.01),
        max_price: m(10000.0),
        order_types: vec![OrderType::Market, OrderType::Limit, OrderType::StopLimit],
    }
}

struct Venue {
    account: String,
    db: SqliteStateManager,
    rules: MarketRules,
    reports: BTreeMap<String, BrokerReport>,
    cash: Money,
    inventory: Money,
    price: Money,
    submits: usize,
    cancels: usize,
    immediate: bool,
    lose_response: bool,
    lose_before_send: bool,
    fill_on_cancel: bool,
    delay_cancel: bool,
    reject: bool,
    stale: bool,
    fail_asset_balance_once: bool,
    fill_during_balance: Option<(String, Money, Money)>,
}

impl Venue {
    fn fill(&mut self, id: &str, quantity: Money, fee: Money) {
        let report = self.reports.get_mut(id).unwrap();
        let old_value = report.filled * report.average_price;
        let value = quantity * self.price;
        let fee_delta = fee - report.fee;
        let delta = quantity - report.filled;
        match report.order.side {
            Side::Buy => {
                self.cash -= value - old_value + fee_delta;
                self.inventory += delta;
            }
            Side::Sell => {
                self.cash += value - old_value - fee_delta;
                self.inventory -= delta;
            }
        }
        report.filled = quantity;
        report.average_price = self.price;
        report.fee = fee;
        report.status = if quantity == report.order.quantity {
            BrokerStatus::Filled
        } else {
            BrokerStatus::PartiallyFilled
        };
        report.updated_at = Utc::now();
    }

    fn locks(&self) -> (Money, Money) {
        let mut cash = Money::ZERO;
        let mut inventory = Money::ZERO;
        for report in self.reports.values().filter(|r| !r.status.terminal()) {
            let remaining = report.order.quantity - report.filled;
            match report.order.side {
                Side::Buy => cash += remaining * report.order.limit.unwrap_or(self.price),
                Side::Sell => inventory += remaining,
            }
        }
        (cash, inventory)
    }
}

#[derive(Clone)]
struct FakeBroker(Arc<Mutex<Venue>>);

impl FakeBroker {
    fn change(&self, f: impl FnOnce(&mut Venue)) {
        f(&mut self.0.lock().unwrap());
    }
    fn reports(&self) -> Vec<BrokerReport> {
        self.0.lock().unwrap().reports.values().cloned().collect()
    }
    fn entries(&self) -> Vec<BrokerReport> {
        self.reports()
            .into_iter()
            .filter(|r| r.order.side == Side::Buy)
            .collect()
    }
    fn stops(&self) -> Vec<BrokerReport> {
        self.reports()
            .into_iter()
            .filter(|r| r.order.kind == OrderType::StopLimit)
            .collect()
    }
}

impl Broker for FakeBroker {
    async fn account_key(&self) -> Result<String> {
        Ok(self.0.lock().unwrap().account.clone())
    }
    async fn markets(&self, _: &[Symbol]) -> Result<Vec<MarketRules>> {
        Ok(vec![self.0.lock().unwrap().rules.clone()])
    }
    async fn balances(&self) -> Result<Vec<Balance>> {
        let mut venue = self.0.lock().unwrap();
        if let Some((id, quantity, fee)) = venue.fill_during_balance.take() {
            venue.fill(&id, quantity, fee);
        }
        if venue.fail_asset_balance_once && venue.inventory.is_positive() {
            venue.fail_asset_balance_once = false;
            bail!("temporary balance read failure");
        }
        let (cash, inventory) = venue.locks();
        Ok(vec![
            Balance {
                currency: "USD".into(),
                available: venue.cash - cash,
                locked: cash,
            },
            Balance {
                currency: "TEST".into(),
                available: venue.inventory - inventory,
                locked: inventory,
            },
        ])
    }
    async fn active_orders(&self, _: &Symbol) -> Result<Vec<BrokerReport>> {
        Ok(self
            .reports()
            .into_iter()
            .filter(|r| !r.status.terminal())
            .collect())
    }
    async fn quotes(&self, _: &[Symbol]) -> Result<Vec<Quote>> {
        let venue = self.0.lock().unwrap();
        Ok(vec![Quote {
            symbol: Symbol::new("TESTUSD"),
            bid: venue.price,
            ask: venue.price,
            timestamp: Utc::now()
                - if venue.stale {
                    Duration::minutes(2)
                } else {
                    Duration::zero()
                },
        }])
    }
    async fn lookup(&self, id: &str) -> Result<Option<BrokerReport>> {
        Ok(self.0.lock().unwrap().reports.get(id).cloned())
    }
    async fn submit(&self, request: &BrokerOrder) -> Result<BrokerReport> {
        let mut venue = self.0.lock().unwrap();
        let snapshot: serde_json::Value = venue.db.load_engine_snapshot()?.unwrap();
        let journaled = snapshot["journal"]["orders"]
            .as_object()
            .unwrap()
            .values()
            .find(|r| r["request"]["client_id"] == request.client_id)
            .unwrap();
        assert_eq!(journaled["phase"], "Submitting");
        assert_eq!(journaled["request"], serde_json::to_value(request)?);
        venue.submits += 1;
        ensure!(
            !venue.reports.contains_key(&request.client_id),
            "Duplicate client ID"
        );
        if venue.lose_before_send {
            venue.lose_before_send = false;
            bail!("connection failed before send");
        }
        let (_, locked) = venue.locks();
        if request.side == Side::Sell {
            ensure!(
                venue.inventory - locked >= request.quantity,
                "Attempt to oversell locked inventory"
            );
        }
        let report = BrokerReport {
            exchange_id: venue.submits.to_string(),
            order: request.clone(),
            status: if venue.reject {
                BrokerStatus::Rejected
            } else if request.kind == OrderType::StopLimit {
                BrokerStatus::Pending
            } else {
                BrokerStatus::Open
            },
            filled: Money::ZERO,
            average_price: Money::ZERO,
            fee: Money::ZERO,
            updated_at: Utc::now(),
        };
        venue.reports.insert(request.client_id.clone(), report);
        if venue.immediate && !venue.reject && request.kind == OrderType::Market {
            let fee = request.quantity * venue.price * m(0.001);
            venue.fill(&request.client_id, request.quantity, fee);
        }
        if venue.lose_response {
            venue.lose_response = false;
            bail!("accepted, but response lost");
        }
        Ok(venue.reports[&request.client_id].clone())
    }
    async fn cancel(&self, id: &str) -> Result<()> {
        let mut venue = self.0.lock().unwrap();
        venue.cancels += 1;
        if venue.delay_cancel {
            return Ok(());
        }
        let report = venue.reports[id].clone();
        if venue.fill_on_cancel && report.order.side == Side::Sell {
            venue.fill_on_cancel = false;
            let fee = report.order.quantity * venue.price * m(0.001);
            venue.fill(id, report.order.quantity, fee);
        } else {
            venue.reports.get_mut(id).unwrap().status = BrokerStatus::Cancelled;
        }
        Ok(())
    }
}

fn fixture() -> (tempfile::TempDir, SqliteStateManager, FakeBroker) {
    let temp = tempfile::tempdir().unwrap();
    let db = SqliteStateManager::new(
        temp.path().join("live.db"),
        temp.path().join("live.json"),
        false,
    )
    .unwrap();
    let broker = FakeBroker(Arc::new(Mutex::new(Venue {
        account: db.execution_namespace().unwrap(),
        db: db.clone(),
        rules: rules(),
        reports: BTreeMap::new(),
        cash: m(1000.0),
        inventory: Money::ZERO,
        price: m(100.0),
        submits: 0,
        cancels: 0,
        immediate: true,
        lose_response: false,
        lose_before_send: false,
        fill_on_cancel: false,
        delay_cancel: false,
        reject: false,
        stale: false,
        fail_asset_balance_once: false,
        fill_during_balance: None,
    })));
    (temp, db, broker)
}

async fn open(db: &SqliteStateManager, broker: &FakeBroker) -> LiveExecution<FakeBroker> {
    LiveExecution::open(config(), Box::new(Probe), broker.clone(), db.clone())
        .await
        .unwrap()
}

async fn decide(live: &mut LiveExecution<FakeBroker>) {
    let data = frame();
    let timestamp = data.values().next().unwrap().primary()[0].datetime;
    live.on_bar(&data, timestamp).await.unwrap();
}

#[tokio::test]
async fn external_decisions_never_simulate_fills_and_preflight_never_mutates_venue() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    live.preflight().await.unwrap();
    decide(&mut live).await;
    assert_eq!(live.engine().cash(), 1000.0);
    assert_eq!(live.engine().positions().open_position_count(), 0);
    assert_eq!(broker.0.lock().unwrap().submits, 0);
    assert_eq!(broker.0.lock().unwrap().cancels, 0);
    live.cycle().await.unwrap();
    assert_eq!(broker.entries().len(), 1);
    assert_eq!(broker.stops().len(), 1);
    assert_eq!(
        live.engine()
            .positions()
            .get_position(&Symbol::new("TESTUSD"))
            .unwrap()
            .quantity,
        m(2.0)
    );
    assert!((live.engine().cash() - 799.8).abs() < 1e-8);
}

#[tokio::test]
async fn lost_submit_response_is_reconciled_after_restart_without_resubmission() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    broker.change(|v| v.lose_response = true);
    live.cycle().await.unwrap();
    assert!(live.halted().is_some());
    assert_eq!(broker.entries().len(), 1);
    assert_eq!(live.engine().positions().open_position_count(), 0);
    drop(live);
    let mut recovered = open(&db, &broker).await;
    recovered.cycle().await.unwrap();
    assert_eq!(broker.entries().len(), 1);
    assert_eq!(broker.stops().len(), 1);
    assert_eq!(recovered.engine().positions().open_position_count(), 1);
    let cash = recovered.engine().cash();
    recovered.cycle().await.unwrap();
    assert_eq!(recovered.engine().cash(), cash);
    recovered.resume().await.unwrap();
    assert!(recovered.halted().is_none());
}

#[tokio::test]
async fn a_persisted_send_intent_is_not_retried_even_when_order_is_absent() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    broker.change(|v| v.lose_before_send = true);
    live.cycle().await.unwrap();
    drop(live);
    let mut recovered = open(&db, &broker).await;
    recovered.cycle().await.unwrap();
    assert_eq!(broker.0.lock().unwrap().submits, 1);
    assert!(recovered.halted().is_some());
    assert!(recovered.resume().await.is_err());
}

#[tokio::test]
async fn cancellation_acknowledgement_does_not_release_inventory_or_send_an_exit() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    broker.change(|v| {
        v.price = m(131.0);
        v.delay_cancel = true;
    });
    live.cycle().await.unwrap();
    assert_eq!(broker.reports().len(), 2);
    assert_eq!(live.engine().positions().open_position_count(), 1);
    live.cycle().await.unwrap();
    assert_eq!(broker.reports().len(), 2);
    broker.change(|v| {
        let stop = v
            .reports
            .values_mut()
            .find(|r| r.order.kind == OrderType::StopLimit)
            .unwrap();
        stop.status = BrokerStatus::Cancelled;
    });
    live.cycle().await.unwrap();
    assert_eq!(live.engine().positions().open_position_count(), 0);
    assert_eq!(live.engine().trades().len(), 1);
}

#[tokio::test]
async fn native_stop_fill_racing_cancellation_cannot_create_a_second_sell() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    broker.change(|v| {
        v.price = m(131.0);
        v.fill_on_cancel = true;
    });
    live.cycle().await.unwrap();
    live.cycle().await.unwrap();
    assert_eq!(live.engine().positions().open_position_count(), 0);
    assert_eq!(broker.reports().len(), 2);
    assert_eq!(live.engine().trades().len(), 1);
}

#[tokio::test]
async fn partial_entry_is_cancelled_and_actual_inventory_is_protected() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| v.immediate = false);
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| v.fill(&id, m(0.5), m(0.05)));
    live.cycle().await.unwrap();
    assert_eq!(broker.stops()[0].order.quantity, m(0.5));
    live.cycle().await.unwrap();
    assert_eq!(broker.entries()[0].status, BrokerStatus::Cancelled);
    assert_eq!(
        live.engine()
            .positions()
            .get_position(&Symbol::new("TESTUSD"))
            .unwrap()
            .quantity,
        m(0.5)
    );
    assert!((live.engine().cash() - 949.95).abs() < 1e-8);
    drop(live);
    let mut recovered = open(&db, &broker).await;
    recovered.preflight().await.unwrap();
}

#[tokio::test]
async fn late_fee_only_updates_correct_closed_fifo_trade_and_cash_exactly_once() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    broker.change(|v| v.price = m(131.0));
    live.cycle().await.unwrap();
    live.cycle().await.unwrap();
    let pnl = live.engine().trades()[0].net_pnl;
    let cash = live.engine().cash();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| {
        v.reports.get_mut(&id).unwrap().fee += m(0.8);
        v.cash -= m(0.8);
    });
    live.cycle().await.unwrap();
    assert_eq!(live.engine().trades()[0].net_pnl, pnl - m(0.8));
    assert!((live.engine().cash() - cash + 0.8).abs() < 1e-8);
    live.cycle().await.unwrap();
    assert_eq!(live.engine().trades()[0].net_pnl, pnl - m(0.8));
}

#[tokio::test]
async fn late_entry_fee_is_allocated_to_remaining_fifo_inventory() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| {
        v.reports.get_mut(&id).unwrap().fee += m(0.8);
        v.cash -= m(0.8);
    });
    live.cycle().await.unwrap();
    let position = live
        .engine()
        .positions()
        .get_position(&Symbol::new("TESTUSD"))
        .unwrap();
    assert_eq!(
        position.fills.iter().map(|f| f.commission).sum::<Money>(),
        m(1.0)
    );
    assert!((live.engine().cash() - 799.0).abs() < 1e-8);
}

#[tokio::test]
async fn unsupported_native_stops_and_insufficient_wallet_funds_fail_before_send() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| {
        v.rules
            .order_types
            .retain(|kind| *kind != OrderType::StopLimit)
    });
    assert!(
        LiveExecution::open(config(), Box::new(Probe), broker.clone(), db.clone())
            .await
            .is_err()
    );
    assert_eq!(broker.0.lock().unwrap().submits, 0);
    broker.change(|v| {
        v.rules = rules();
        v.cash = m(500.0);
    });
    assert!(
        LiveExecution::open(config(), Box::new(Probe), broker.clone(), db)
            .await
            .is_err()
    );
    assert_eq!(broker.0.lock().unwrap().submits, 0);
}

#[tokio::test]
async fn wallet_discrepancy_halts_entries_without_overwriting_ledger_or_cancelling_stops() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let cash = live.engine().cash();
    broker.change(|v| v.cash -= m(1.0));
    live.cycle().await.unwrap();
    assert!(live.halted().unwrap().contains("Account reconciliation"));
    assert_eq!(live.engine().cash(), cash);
    assert_eq!(broker.stops()[0].status, BrokerStatus::Pending);
}

#[tokio::test]
async fn rejected_entry_and_stale_quotes_cannot_create_positions() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    broker.change(|v| v.stale = true);
    assert!(live.cycle().await.is_err());
    assert!(broker.reports().is_empty());
    broker.change(|v| {
        v.stale = false;
        v.reject = true;
    });
    live.cycle().await.unwrap();
    assert!(live.halted().is_some());
    assert_eq!(live.engine().positions().open_position_count(), 0);
}

#[tokio::test]
async fn shutdown_cancels_entries_and_keeps_exchange_held_protection() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    live.prepare_shutdown().await.unwrap();
    live.cycle().await.unwrap();
    assert!(live.shutdown_ready());
    assert_eq!(broker.stops()[0].status, BrokerStatus::Pending);
    drop(live);
    let mut recovered = open(&db, &broker).await;
    assert!(recovered.halted().is_some());
    recovered.resume().await.unwrap();
    assert!(recovered.halted().is_none());
}

#[test]
fn exact_decimal_rules_never_round_quantity_up() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let rule = rules();
    let quantity = Money::from(Decimal::from_str("1.2399999999999999999999999999").unwrap());
    assert_eq!(rule.quantity(quantity), m(1.23));
    assert_eq!(rule.price(m(12.341), true), m(12.35));
    assert_eq!(rule.price(m(12.349), false), m(12.34));
    let engine = TradingEngine::new_external_spot(config(), Box::new(Probe)).unwrap();
    assert!(engine.execution_intents().is_empty());
}

#[tokio::test]
async fn missing_quotes_do_not_prevent_cancelling_halted_entries() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| v.immediate = false);
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    live.halt("operator halt").await.unwrap();
    broker.change(|v| v.stale = true);
    assert!(live.cycle().await.is_err());
    assert_eq!(broker.entries()[0].status, BrokerStatus::Cancelled);
    assert!(live.cycle().await.is_err());
    assert!(live.engine().execution_intents().is_empty());
}

#[tokio::test]
async fn recovered_unjournaled_protection_is_submitted_even_when_quotes_are_stale() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    broker.change(|v| v.fail_asset_balance_once = true);
    assert!(live.cycle().await.is_err());
    assert_eq!(broker.entries().len(), 1);
    assert!(broker.stops().is_empty());
    live.halt("balance read failure").await.unwrap();
    drop(live);
    broker.change(|v| v.stale = true);
    let mut recovered = open(&db, &broker).await;
    assert!(recovered.cycle().await.is_err());
    assert_eq!(broker.stops().len(), 1);
    assert_eq!(broker.stops()[0].status, BrokerStatus::Pending);
}

#[tokio::test]
async fn newly_reconciled_partial_inventory_gets_protection_before_quote_failure() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| v.immediate = false);
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| {
        v.fill(&id, m(0.5), m(0.05));
        v.stale = true;
    });
    assert!(live.cycle().await.is_err());
    assert_eq!(broker.stops()[0].order.quantity, m(0.5));
}

#[tokio::test]
async fn dust_partial_is_an_explicit_failure_not_a_successful_protective_order() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| v.immediate = false);
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| v.fill(&id, m(0.001), m(0.0001)));
    let result = live.cycle().await;
    assert!(result.is_err() || live.halted().is_some());
    assert!(broker.stops().is_empty());
    assert_eq!(
        live.engine()
            .positions()
            .get_position(&Symbol::new("TESTUSD"))
            .unwrap()
            .quantity,
        m(0.001)
    );
    assert!(!live.shutdown_ready());
}

#[tokio::test]
async fn live_account_lock_excludes_a_second_state_database() {
    let (_temp, db, broker) = fixture();
    let _live = open(&db, &broker).await;
    let other = tempfile::tempdir().unwrap();
    let other_db = SqliteStateManager::new(
        other.path().join("other.db"),
        other.path().join("other.json"),
        false,
    )
    .unwrap();
    let result = LiveExecution::open(config(), Box::new(Probe), broker.clone(), other_db).await;
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("Another local process"));
    assert!(broker.reports().is_empty());
}

#[tokio::test]
async fn a_different_account_or_configuration_cannot_reuse_live_state() {
    let (_temp, db, broker) = fixture();
    let live = open(&db, &broker).await;
    drop(live);
    let mut changed = config();
    changed.trading.initial_capital = 999.0;
    assert!(
        LiveExecution::open(changed, Box::new(Probe), broker.clone(), db.clone())
            .await
            .is_err()
    );
    broker.change(|v| v.account.push_str("-another-account"));
    assert!(LiveExecution::open(config(), Box::new(Probe), broker, db)
        .await
        .is_err());
}

#[tokio::test]
async fn late_terminal_fee_beyond_poll_window_recovers_across_restart() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    broker.change(|v| v.price = m(131.0));
    live.cycle().await.unwrap();
    live.cycle().await.unwrap();
    let pnl = live.engine().trades()[0].net_pnl;
    let cash = live.engine().cash();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| v.reports.get_mut(&id).unwrap().updated_at = Utc::now() - Duration::days(1));
    live.cycle().await.unwrap();
    drop(live);
    broker.change(|v| {
        v.reports.get_mut(&id).unwrap().fee += m(0.8);
        v.cash -= m(0.8);
    });
    let mut recovered = open(&db, &broker).await;
    recovered.preflight().await.unwrap();
    assert!(recovered.halted().is_none());
    assert_eq!(recovered.engine().trades()[0].net_pnl, pnl - m(0.8));
    assert!((recovered.engine().cash() - cash + 0.8).abs() < 1e-8);
    recovered.cycle().await.unwrap();
    assert_eq!(recovered.engine().trades()[0].net_pnl, pnl - m(0.8));
}

#[tokio::test]
async fn extra_fill_during_cancel_replaces_undersized_stop_without_fresh_quotes() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| {
        v.immediate = false;
        v.delay_cancel = true;
    });
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| v.fill(&id, m(0.5), m(0.05)));
    live.cycle().await.unwrap();
    assert_eq!(broker.stops()[0].order.quantity, m(0.5));
    broker.change(|v| {
        v.fill(&id, m(1.0), m(0.1));
        v.reports.get_mut(&id).unwrap().status = BrokerStatus::Cancelled;
        v.stale = true;
        v.delay_cancel = false;
    });
    assert!(live.cycle().await.is_err());
    assert_eq!(broker.stops()[0].status, BrokerStatus::Cancelled);
    assert!(live.cycle().await.is_err());
    let active: Vec<_> = broker
        .stops()
        .into_iter()
        .filter(|r| !r.status.terminal())
        .collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].order.quantity, m(1.0));
}

#[tokio::test]
async fn an_invalid_exit_never_cancels_an_existing_native_stop() {
    let (_temp, db, broker) = fixture();
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    drop(live);
    broker.change(|v| {
        v.rules.max_market_quantity = m(1.0);
        v.price = m(131.0);
    });
    let mut recovered = open(&db, &broker).await;
    recovered.cycle().await.unwrap();
    assert!(recovered.halted().is_some());
    assert_eq!(broker.stops()[0].status, BrokerStatus::Pending);
    assert_eq!(broker.0.lock().unwrap().cancels, 0);
}

#[tokio::test]
async fn unprotectable_partial_can_still_take_a_legal_risk_reducing_exit() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| v.immediate = false);
    let mut live = open(&db, &broker).await;
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| v.fill(&id, m(0.01), m(0.001)));
    live.cycle().await.unwrap();
    assert!(live.halted().is_some());
    assert!(broker.stops().is_empty());
    broker.change(|v| {
        v.price = m(131.0);
        v.immediate = true;
    });
    live.cycle().await.unwrap();
    assert_eq!(live.engine().positions().open_position_count(), 0);
    assert_eq!(live.engine().trades().len(), 1);
}

#[derive(Clone)]
struct TwoMarkets([FakeBroker; 2]);

impl TwoMarkets {
    fn venue(&self, symbol: &Symbol) -> &FakeBroker {
        &self.0[usize::from(symbol.as_str() == "ALTUSD")]
    }
}

impl Broker for TwoMarkets {
    async fn account_key(&self) -> Result<String> {
        self.0[0].account_key().await
    }
    async fn markets(&self, symbols: &[Symbol]) -> Result<Vec<MarketRules>> {
        let mut markets = self.0[0].markets(symbols).await?;
        markets.extend(self.0[1].markets(symbols).await?);
        Ok(markets)
    }
    async fn balances(&self) -> Result<Vec<Balance>> {
        let first = self.0[0].balances().await?;
        let second = self.0[1].balances().await?;
        Ok(vec![
            Balance {
                currency: "USD".into(),
                available: first[0].available + second[0].available,
                locked: first[0].locked + second[0].locked,
            },
            first[1].clone(),
            Balance {
                currency: "ALT".into(),
                ..second[1].clone()
            },
        ])
    }
    async fn active_orders(&self, symbol: &Symbol) -> Result<Vec<BrokerReport>> {
        self.venue(symbol).active_orders(symbol).await
    }
    async fn quotes(&self, symbols: &[Symbol]) -> Result<Vec<Quote>> {
        let mut quotes = Vec::new();
        for symbol in symbols {
            let mut quote = self
                .venue(symbol)
                .quotes(std::slice::from_ref(symbol))
                .await?
                .remove(0);
            quote.symbol = symbol.clone();
            quotes.push(quote);
        }
        Ok(quotes)
    }
    async fn lookup(&self, id: &str) -> Result<Option<BrokerReport>> {
        for broker in &self.0 {
            if let Some(report) = broker.lookup(id).await? {
                return Ok(Some(report));
            }
        }
        Ok(None)
    }
    async fn submit(&self, order: &BrokerOrder) -> Result<BrokerReport> {
        self.venue(&order.symbol).submit(order).await
    }
    async fn cancel(&self, id: &str) -> Result<()> {
        for broker in &self.0 {
            if broker.lookup(id).await?.is_some() {
                return broker.cancel(id).await;
            }
        }
        bail!("Unknown cancellation")
    }
}

#[tokio::test]
async fn one_unprotectable_symbol_does_not_starve_other_symbols_protection() {
    let (_first_temp, db, first) = fixture();
    let (_second_temp, _second_db, second) = fixture();
    first.change(|v| v.immediate = false);
    second.change(|v| {
        v.db = db.clone();
        v.immediate = false;
        v.rules.symbol = Symbol::new("ALTUSD");
        v.rules.asset = "ALT".into();
    });
    let broker = TwoMarkets([first.clone(), second.clone()]);
    let mut cfg = config();
    cfg.trading.symbols = vec!["ALTUSD".into(), "TESTUSD".into()];
    cfg.trading.max_positions = 2;
    let mut live = LiveExecution::open(cfg, Box::new(Probe), broker, db)
        .await
        .unwrap();
    let mut input = frame();
    input.insert(
        Symbol::new("ALTUSD"),
        input[&Symbol::new("TESTUSD")].clone(),
    );
    let timestamp = input.values().next().unwrap().primary()[0].datetime;
    live.on_bar(&input, timestamp).await.unwrap();
    live.cycle().await.unwrap();
    live.cycle().await.unwrap();
    let good = first.entries()[0].order.client_id.clone();
    let dust = second.entries()[0].order.client_id.clone();
    first.change(|v| v.fill(&good, m(0.5), m(0.05)));
    second.change(|v| v.fill(&dust, m(0.001), m(0.0001)));
    live.cycle().await.unwrap();
    assert!(live.halted().is_some());
    assert_eq!(first.stops()[0].order.quantity, m(0.5));
    assert!(second.stops().is_empty());
}

#[derive(Clone)]
struct FailingTrail;

impl Strategy for FailingTrail {
    fn name(&self) -> &'static str {
        "failing-trail"
    }
    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(self.clone())
    }
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        Probe.generate_orders(ctx)
    }
    fn calculate_stop_loss(&self, candles: &[Candle], price: f64, side: Side) -> f64 {
        Probe.calculate_stop_loss(candles, price, side)
    }
    fn calculate_take_profit(&self, candles: &[Candle], price: f64, side: Side) -> f64 {
        Probe.calculate_take_profit(candles, price, side)
    }
    fn update_trailing_stop(&self, _: &Position, _: f64, _: &[Candle]) -> Option<f64> {
        Some(f64::NAN)
    }
}

#[tokio::test]
async fn failed_strategy_decision_halts_but_reconciliation_and_exits_continue() {
    let (_temp, db, broker) = fixture();
    let mut live = LiveExecution::open(config(), Box::new(FailingTrail), broker.clone(), db)
        .await
        .unwrap();
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let input = frame();
    let timestamp = input.values().next().unwrap().primary()[0].datetime;
    assert!(live.on_bar(&input, timestamp).await.is_err());
    assert!(live.halted().is_some());
    assert_eq!(live.engine().positions().open_position_count(), 1);
    assert_eq!(broker.stops()[0].status, BrokerStatus::Pending);
    broker.change(|v| v.price = m(131.0));
    live.cycle().await.unwrap();
    live.cycle().await.unwrap();
    assert_eq!(live.engine().positions().open_position_count(), 0);
    assert_eq!(live.engine().trades().len(), 1);
}

#[derive(Clone)]
struct TwoEntries;

impl Strategy for TwoEntries {
    fn name(&self) -> &'static str {
        "two-entries"
    }
    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(self.clone())
    }
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut requests = Probe.generate_orders(ctx);
        requests.extend(Probe.generate_orders(ctx));
        requests
    }
    fn calculate_stop_loss(&self, candles: &[Candle], price: f64, side: Side) -> f64 {
        Probe.calculate_stop_loss(candles, price, side)
    }
    fn calculate_take_profit(&self, candles: &[Candle], price: f64, side: Side) -> f64 {
        Probe.calculate_take_profit(candles, price, side)
    }
    fn update_trailing_stop(&self, _: &Position, _: f64, _: &[Candle]) -> Option<f64> {
        None
    }
}

#[tokio::test]
async fn wallet_audit_new_fill_is_protected_before_any_additional_entry() {
    let (_temp, db, broker) = fixture();
    broker.change(|v| v.immediate = false);
    let mut live = LiveExecution::open(config(), Box::new(TwoEntries), broker.clone(), db)
        .await
        .unwrap();
    decide(&mut live).await;
    live.cycle().await.unwrap();
    let id = broker.entries()[0].order.client_id.clone();
    broker.change(|v| v.fill_during_balance = Some((id, m(2.0), m(0.2))));
    live.cycle().await.unwrap();
    assert_eq!(broker.entries().len(), 1);
    assert_eq!(broker.stops()[0].order.quantity, m(2.0));
    assert!(live.halted().is_none());
    live.cycle().await.unwrap();
    assert_eq!(broker.entries().len(), 2);
}
