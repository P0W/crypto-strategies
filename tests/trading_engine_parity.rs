use chrono::{DateTime, Duration, TimeZone, Utc};
use crypto_strategies::backtest::Backtester;
use crypto_strategies::multi_timeframe::align_multi_timeframe_data;
use crypto_strategies::oms::trading_engine::CumulativeFill;
use crypto_strategies::oms::{EngineSnapshot, TradingEngine};
use crypto_strategies::state_manager::SqliteStateManager;
use crypto_strategies::{
    Candle, Config, Fill, Money, MultiSymbolMultiTimeframeData, MultiTimeframeData, OrderRequest,
    Position, Side, Strategy, StrategyContext, Symbol, Trade,
};
use std::sync::{Arc, Mutex};

fn time(hour: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::hours(hour)
}

fn candle(hour: i64, open: f64, high: f64, low: f64, close: f64) -> Candle {
    Candle::new(time(hour), open, high, low, close, 1000.0).unwrap()
}

fn config(t1: bool) -> Config {
    serde_json::from_value(serde_json::json!({
        "exchange": {"maker_fee": 0.001, "taker_fee": 0.002, "assumed_slippage": 0.001, "rate_limit": 10},
        "trading": {
            "symbols": ["TEST"], "initial_capital": 1000.0, "risk_per_trade": 0.5,
            "max_positions": 1, "max_portfolio_heat": 1.0, "max_position_pct": 1.0,
            "max_drawdown": 1.0, "drawdown_warning": 0.9, "drawdown_critical": 0.95,
            "drawdown_warning_multiplier": 1.0, "drawdown_critical_multiplier": 1.0,
            "consecutive_loss_limit": 100, "consecutive_loss_multiplier": 1.0
        },
        "strategy": {"name": "review_probe", "timeframe": "1h"},
        "tax": {"tax_rate": 0.0, "tds_rate": 0.0, "loss_offset_allowed": true},
        "backtest": {"data_dir": "data", "results_dir": "results", "commission": 0.0,
            "use_t1_execution": t1}
    })).unwrap()
}

fn data(candles: Vec<Candle>) -> MultiSymbolMultiTimeframeData {
    let mut mtf = MultiTimeframeData::new("1h");
    mtf.add_timeframe("1h", candles);
    MultiSymbolMultiTimeframeData::from([(Symbol::new("TEST"), mtf)])
}

#[derive(Clone, Copy)]
enum Entry {
    Market,
    Limit,
    Short,
    None,
}

#[derive(Clone)]
struct Probe {
    entry: Entry,
    trail: bool,
    higher: bool,
    bars: usize,
    events: Arc<Mutex<Vec<String>>>,
}

impl Probe {
    fn new(entry: Entry) -> Self {
        Self {
            entry,
            trail: false,
            higher: false,
            bars: 0,
            events: Arc::default(),
        }
    }
}

impl Strategy for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(self.clone())
    }
    fn required_timeframes(&self) -> Vec<&'static str> {
        if self.higher {
            vec!["1d"]
        } else {
            vec![]
        }
    }
    fn snapshot_state(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::to_value(self.bars)?)
    }
    fn restore_state(&mut self, state: serde_json::Value) -> anyhow::Result<()> {
        self.bars = serde_json::from_value(state)?;
        Ok(())
    }
    fn on_bar(&mut self, ctx: &StrategyContext) {
        self.bars += 1;
        self.events
            .lock()
            .unwrap()
            .push(format!("bar:{}:{}", ctx.symbol, self.bars));
    }
    fn on_order_filled(&mut self, fill: &Fill, position: &Position) {
        self.events.lock().unwrap().push(format!(
            "fill:{}:{}:{}:{}",
            position.symbol, fill.price, fill.quantity, fill.is_maker
        ));
    }
    fn on_trade_closed(&mut self, trade: &Trade) {
        self.events
            .lock()
            .unwrap()
            .push(format!("closed:{}", trade.net_pnl));
    }
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        if self.higher {
            if let Some(day) = ctx.get_timeframe("1d").and_then(|bars| bars.last()) {
                assert!(
                    day.datetime + Duration::days(1)
                        <= ctx.candles.last().unwrap().datetime + Duration::hours(1)
                );
                self.events.lock().unwrap().push("closed_daily".into());
            }
        }
        if ctx.candles.len() != 1 {
            return vec![];
        }
        let request = match self.entry {
            Entry::Market => OrderRequest::market_buy(ctx.symbol.clone(), 2.0),
            Entry::Limit => OrderRequest::limit_buy(ctx.symbol.clone(), 2.0, 95.0),
            Entry::Short => OrderRequest::market_sell(ctx.symbol.clone(), 2.0),
            Entry::None => return vec![],
        };
        vec![request.with_quantity_cap()]
    }
    fn calculate_stop_loss(&self, _: &[Candle], _: f64, side: Side) -> f64 {
        if side == Side::Buy {
            0.0
        } else {
            1000.0
        }
    }
    fn calculate_take_profit(&self, _: &[Candle], _: f64, side: Side) -> f64 {
        if side == Side::Buy {
            1000.0
        } else {
            0.01
        }
    }
    fn update_trailing_stop(&self, _: &Position, price: f64, _: &[Candle]) -> Option<f64> {
        (self.trail && price > 100.0).then_some(price - 10.0)
    }
}

#[test]
fn backtest_and_incremental_paper_have_identical_ledger_and_callbacks() {
    for t1 in [false, true] {
        for entry in [Entry::Market, Entry::Limit, Entry::Short] {
            let cfg = config(t1);
            let input = data(vec![
                candle(0, 100.0, 101.0, 99.0, 100.0),
                candle(1, 102.0, 110.0, 94.0, 108.0),
                candle(2, 109.0, 120.0, 108.0, 118.0),
                candle(3, 118.0, 125.0, 115.0, 120.0),
            ]);
            let historical = Probe::new(entry);
            let historical_events = historical.events.clone();
            let result = Backtester::new(cfg.clone(), Box::new(historical))
                .run(&input)
                .unwrap();
            let streaming = Probe::new(entry);
            let streaming_events = streaming.events.clone();
            let mut engine = TradingEngine::new(cfg, Box::new(streaming)).unwrap();
            let mut equities = Vec::new();
            for hour in 0..4 {
                let prefix = data(input.values().next().unwrap().primary()[..=hour].to_vec());
                engine.on_bar(&prefix, time(hour as i64)).unwrap();
                equities.push((time(hour as i64), engine.equity()));
            }
            engine.finish(time(3)).unwrap();
            equities.last_mut().unwrap().1 = engine.equity();
            assert_eq!(result.equity_curve, equities);
            assert_eq!(
                serde_json::to_value(result.trades).unwrap(),
                serde_json::to_value(engine.trades()).unwrap()
            );
            assert_eq!(
                *historical_events.lock().unwrap(),
                *streaming_events.lock().unwrap()
            );
        }
    }
}

#[test]
fn aligned_symbols_have_exactly_the_same_timestamps() {
    let mut input = MultiSymbolMultiTimeframeData::new();
    for (symbol, hours) in [("A", vec![0, 1, 2, 3]), ("B", vec![3, 2, 0])] {
        let mut mtf = MultiTimeframeData::new("1h");
        mtf.add_timeframe(
            "1h",
            hours
                .into_iter()
                .map(|h| candle(h, 100.0, 100.0, 100.0, 100.0))
                .collect(),
        );
        input.insert(Symbol::new(symbol), mtf);
    }
    let aligned = align_multi_timeframe_data(&input);
    assert_eq!(aligned.len(), 2);
    assert_eq!(aligned[0].1.primary().len(), 3);
    for (a, b) in aligned[0].1.primary().iter().zip(aligned[1].1.primary()) {
        assert_eq!(a.datetime, b.datetime);
    }
}

#[test]
fn daily_candle_is_available_only_after_its_close() {
    let mut probe = Probe::new(Entry::None);
    probe.higher = true;
    let events = probe.events.clone();
    let mut input = data(
        (0..25)
            .map(|h| candle(h, 100.0, 100.0, 100.0, 100.0))
            .collect(),
    );
    input
        .values_mut()
        .next()
        .unwrap()
        .add_timeframe("1d", vec![candle(0, 100.0, 200.0, 100.0, 200.0)]);
    Backtester::new(config(false), Box::new(probe))
        .run(&input)
        .unwrap();
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| *event == "closed_daily")
            .count(),
        2
    );
}

#[test]
fn close_based_trailing_stop_is_not_retroactive() {
    let mut cfg = config(false);
    cfg.exchange.assumed_slippage = 0.0;
    cfg.exchange.maker_fee = 0.0;
    cfg.exchange.taker_fee = 0.0;
    let mut probe = Probe::new(Entry::Market);
    probe.trail = true;
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 120.0, 100.0, 120.0),
        candle(2, 130.0, 130.0, 130.0, 130.0),
    ]);
    let result = Backtester::new(cfg, Box::new(probe)).run(&input).unwrap();
    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.trades[0].exit_time, time(2));
    assert_eq!(result.trades[0].exit_price, Money::from_f64(130.0));
    assert_eq!(result.equity_curve.last().unwrap().1, 1060.0);
}

#[test]
fn partial_fills_use_incremental_not_cumulative_price() {
    let mut engine = TradingEngine::new(config(true), Box::new(Probe::new(Entry::Market))).unwrap();
    engine
        .on_bar(&data(vec![candle(0, 100.0, 100.0, 100.0, 100.0)]), time(0))
        .unwrap();
    let id = engine.open_orders().next().unwrap().id;
    for (qty, average) in [(1.0, 100.0), (1.0, 100.0), (2.0, 150.0)] {
        engine
            .apply_cumulative_fill(CumulativeFill {
                order_id: id,
                quantity: Money::from_f64(qty),
                average_price: Money::from_f64(average),
                incremental_commission: Money::from_f64(1.0),
                is_maker: false,
                timestamp: time(1),
            })
            .unwrap();
    }
    let pos = engine
        .positions()
        .get_position(&Symbol::new("TEST"))
        .unwrap();
    assert_eq!(pos.quantity, Money::from_f64(2.0));
    assert_eq!(pos.average_entry_price, Money::from_f64(150.0));
    assert_eq!(engine.cash(), 698.0);
    assert_eq!(engine.open_orders().count(), 0);
}

#[test]
fn liquidation_preserves_exact_fractional_position_quantity() {
    let quantity = Money::from_f64(0.3) + Money::from_f64(5e-17);
    for entry in [Entry::Market, Entry::Short] {
        let mut engine = TradingEngine::new(config(true), Box::new(Probe::new(entry))).unwrap();
        engine
            .on_bar(&data(vec![candle(0, 100.0, 100.0, 100.0, 100.0)]), time(0))
            .unwrap();
        let id = engine.open_orders().next().unwrap().id;
        for cumulative in [Money::from_f64(0.1), Money::from_f64(0.2), quantity] {
            engine
                .apply_cumulative_fill(CumulativeFill {
                    order_id: id,
                    quantity: cumulative,
                    average_price: Money::from_f64(100.0),
                    incremental_commission: Money::ZERO,
                    is_maker: false,
                    timestamp: time(0),
                })
                .unwrap();
        }
        engine.finish(time(1)).unwrap();
        assert_eq!(engine.positions().open_position_count(), 0);
        assert_eq!(engine.open_orders().count(), 0);
        assert_eq!(engine.trades().last().unwrap().quantity, quantity);
    }
}

#[test]
fn snapshot_preserves_pending_orders_fees_cooldown_and_risk() {
    let cfg = config(true);
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 110.0, 110.0, 110.0, 110.0),
        candle(2, 120.0, 120.0, 120.0, 120.0),
    ]);
    let mut original =
        TradingEngine::new(cfg.clone(), Box::new(Probe::new(Entry::Market))).unwrap();
    original.on_bar(&input, time(0)).unwrap();
    let snapshot = original.snapshot().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let db = SqliteStateManager::new(
        directory.path().join("state.db"),
        directory.path().join("state.json"),
        true,
    )
    .unwrap();
    db.save_engine_snapshot(&snapshot).unwrap();
    let mut restored = TradingEngine::new(cfg, Box::new(Probe::new(Entry::Market))).unwrap();
    restored
        .restore(
            db.load_engine_snapshot::<EngineSnapshot>()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        restored.open_orders().next().unwrap().id,
        original.open_orders().next().unwrap().id
    );
    for hour in [1, 2] {
        original.on_bar(&input, time(hour)).unwrap();
        restored.on_bar(&input, time(hour)).unwrap();
        assert_eq!(original.equity(), restored.equity());
        assert_eq!(
            original.risk().peak_capital(),
            restored.risk().peak_capital()
        );
        assert_eq!(
            original.risk().current_drawdown(),
            restored.risk().current_drawdown()
        );
    }
    original.finish(time(2)).unwrap();
    restored.finish(time(2)).unwrap();
    assert_eq!(
        serde_json::to_value(original.trades()).unwrap(),
        serde_json::to_value(restored.trades()).unwrap()
    );
    db.save_engine_snapshot(&restored.snapshot().unwrap())
        .unwrap();
    restored
        .restore(
            db.load_engine_snapshot::<EngineSnapshot>()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        restored.positions().open_position_count(),
        0,
        "Closed position resurrected"
    );
    assert_eq!(restored.open_orders().count(), 0);
}

#[test]
fn drawdown_marks_and_heat_update_without_restart() {
    let mut cfg = config(false);
    cfg.trading.max_drawdown = 0.05;
    cfg.trading.drawdown_warning = 0.03;
    cfg.trading.drawdown_critical = 0.04;
    cfg.exchange.assumed_slippage = 0.0;
    cfg.exchange.maker_fee = 0.0;
    cfg.exchange.taker_fee = 0.0;
    let mut engine = TradingEngine::new(cfg, Box::new(Probe::new(Entry::Market))).unwrap();
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 60.0, 60.0, 60.0, 60.0),
    ]);
    engine.on_bar(&input, time(0)).unwrap();
    assert_eq!(
        engine
            .positions()
            .get_position(&Symbol::new("TEST"))
            .unwrap()
            .risk_amount
            .to_f64(),
        200.0
    );
    engine.on_bar(&input, time(1)).unwrap();
    assert_eq!(engine.equity(), 920.0);
    assert_eq!(engine.risk().current_capital, 920.0);
    assert!(engine.risk().should_halt_trading());
}

#[test]
fn pending_entries_reserve_position_slots() {
    let mut input = data(vec![candle(0, 100.0, 100.0, 100.0, 100.0)]);
    input.insert(Symbol::new("OTHER"), input.values().next().unwrap().clone());
    let mut engine = TradingEngine::new(config(true), Box::new(Probe::new(Entry::Market))).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    assert_eq!(engine.open_orders().count(), 1);
}

#[test]
fn repeated_candle_is_idempotent_and_older_candle_is_rejected() {
    let input = data(vec![candle(0, 100.0, 100.0, 100.0, 100.0)]);
    let mut engine =
        TradingEngine::new(config(false), Box::new(Probe::new(Entry::Market))).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    assert!(engine.on_bar(&input, time(-1)).is_err());
    assert_eq!(
        before,
        serde_json::to_value(engine.snapshot().unwrap()).unwrap()
    );
}

#[test]
fn built_in_strategies_match_across_adapters_and_restart() {
    use crypto_strategies::strategies::{
        create_strategy, momentum_scalper::MomentumScalperConfig, quick_flip::QuickFlipConfig,
        range_breakout::RangeBreakoutConfig, regime_grid::RegimeGridConfig,
        volatility_regime::VolatilityRegimeConfig,
    };
    let strategies = [
        (
            "volatility_regime",
            serde_json::to_value(VolatilityRegimeConfig::default()).unwrap(),
        ),
        (
            "momentum_scalper",
            serde_json::to_value(MomentumScalperConfig::default()).unwrap(),
        ),
        (
            "quick_flip",
            serde_json::to_value(QuickFlipConfig::default()).unwrap(),
        ),
        (
            "range_breakout",
            serde_json::to_value(RangeBreakoutConfig::default()).unwrap(),
        ),
        (
            "regime_grid",
            serde_json::to_value(RegimeGridConfig::default()).unwrap(),
        ),
    ];
    let prices: Vec<_> = (0..400)
        .map(|hour| 100.0 + hour as f64 * 0.3 + (hour as f64 / 20.0).sin() * 15.0)
        .collect();
    let first = data(
        prices
            .iter()
            .enumerate()
            .map(|(i, &price)| {
                let open = prices[i.saturating_sub(1)];
                candle(
                    i as i64,
                    open,
                    open.max(price) + 1.0,
                    open.min(price) - 1.0,
                    price,
                )
            })
            .collect(),
    );
    let mut input = first.clone();
    input.insert(
        Symbol::new("SECOND"),
        first.values().next().unwrap().clone(),
    );
    let mut total_trades = 0;
    for (name, params) in strategies {
        let mut cfg = config(false);
        cfg.trading.max_positions = 2;
        cfg.strategy = params;
        cfg.strategy["name"] = name.into();
        cfg.strategy["timeframe"] = "1h".into();
        let historical = Backtester::new(cfg.clone(), create_strategy(&cfg).unwrap())
            .run(&input)
            .unwrap();
        let mut streaming =
            TradingEngine::new(cfg.clone(), create_strategy(&cfg).unwrap()).unwrap();
        let mut equities = Vec::new();
        for hour in 0..400 {
            let prefix = input
                .iter()
                .map(|(symbol, mtf)| {
                    let mut partial = MultiTimeframeData::new("1h");
                    partial.add_timeframe("1h", mtf.primary()[..=hour].to_vec());
                    (symbol.clone(), partial)
                })
                .collect();
            streaming.on_bar(&prefix, time(hour as i64)).unwrap();
            equities.push((time(hour as i64), streaming.equity()));
            if hour == 225 {
                let json = serde_json::to_vec(&streaming.snapshot().unwrap()).unwrap();
                let mut restarted =
                    TradingEngine::new(cfg.clone(), create_strategy(&cfg).unwrap()).unwrap();
                restarted
                    .restore(serde_json::from_slice(&json).unwrap())
                    .unwrap();
                streaming = restarted;
            }
        }
        streaming.finish(time(399)).unwrap();
        equities.last_mut().unwrap().1 = streaming.equity();
        assert_eq!(equities, historical.equity_curve, "{name} equity diverged");
        assert_eq!(
            serde_json::to_value(streaming.trades()).unwrap(),
            serde_json::to_value(&historical.trades).unwrap(),
            "{name} ledger diverged"
        );
        total_trades += historical.trades.len();
    }
    assert!(
        total_trades > 0,
        "Parity fixture must exercise actual fills"
    );
}

#[derive(Clone)]
struct ScriptedOrders {
    orders: Vec<Vec<OrderRequest>>,
    stop: f64,
    target: f64,
    cancel_at: Option<usize>,
    closed: Arc<Mutex<Vec<Trade>>>,
}

impl ScriptedOrders {
    fn new(orders: Vec<Vec<OrderRequest>>) -> Self {
        Self {
            orders,
            stop: 90.0,
            target: 1000.0,
            cancel_at: None,
            closed: Arc::default(),
        }
    }
}

impl Strategy for ScriptedOrders {
    fn name(&self) -> &'static str {
        "scripted_orders"
    }
    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(self.clone())
    }
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        self.orders
            .get(ctx.candles.len() - 1)
            .cloned()
            .unwrap_or_default()
    }
    fn orders_to_cancel(&self, ctx: &StrategyContext) -> Vec<u64> {
        if self.cancel_at == Some(ctx.candles.len()) {
            ctx.open_orders.iter().map(|order| order.id).collect()
        } else {
            vec![]
        }
    }
    fn calculate_stop_loss(&self, _: &[Candle], _: f64, side: Side) -> f64 {
        if side == Side::Buy {
            self.stop
        } else {
            self.target
        }
    }
    fn calculate_take_profit(&self, _: &[Candle], _: f64, side: Side) -> f64 {
        if side == Side::Buy {
            self.target
        } else {
            self.stop
        }
    }
    fn update_trailing_stop(&self, _: &Position, _: f64, _: &[Candle]) -> Option<f64> {
        None
    }
    fn on_trade_closed(&mut self, trade: &Trade) {
        self.closed.lock().unwrap().push(trade.clone());
    }
}

fn zero_cost_config(t1: bool) -> Config {
    let mut cfg = config(t1);
    cfg.exchange.assumed_slippage = 0.0;
    cfg.exchange.maker_fee = 0.0;
    cfg.exchange.taker_fee = 0.0;
    cfg
}

fn cumulative(id: u64, quantity: f64, average: f64, fee: f64) -> CumulativeFill {
    CumulativeFill {
        order_id: id,
        quantity: Money::from_f64(quantity),
        average_price: Money::from_f64(average),
        incremental_commission: Money::from_f64(fee),
        is_maker: false,
        timestamp: time(1),
    }
}

#[test]
fn short_protection_realizes_insolvency_instead_of_cancelling_the_cover() {
    for t1 in [false, true] {
        let input = data(vec![
            candle(0, 100.0, 100.0, 100.0, 100.0),
            candle(1, 100.0, 100.0, 100.0, 100.0),
            candle(2, 10000.0, 10000.0, 10000.0, 10000.0),
            candle(3, 10000.0, 10000.0, 10000.0, 10000.0),
        ]);
        let mut engine =
            TradingEngine::new(zero_cost_config(t1), Box::new(Probe::new(Entry::Short))).unwrap();
        for hour in 0..4 {
            engine.on_bar(&input, time(hour)).unwrap();
        }
        assert_eq!(engine.positions().open_position_count(), 0);
        assert_eq!(engine.trades().len(), 1);
        assert!(engine.cash() < 0.0);
        assert!(engine.risk().should_halt_trading());
        engine.finish(time(3)).unwrap();
    }
}

#[test]
fn execution_gaps_revalidate_heat_and_exposure_before_filling() {
    let mut cfg = zero_cost_config(true);
    cfg.trading.risk_per_trade = 0.05;
    cfg.trading.max_portfolio_heat = 0.1;
    cfg.trading.max_position_pct = 0.5;
    let strategy = ScriptedOrders::new(vec![vec![OrderRequest::market_buy(
        Symbol::new("TEST"),
        5.0,
    )
    .with_quantity_cap()]]);
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 150.0, 150.0, 150.0, 150.0),
    ]);
    let mut engine = TradingEngine::new(cfg, Box::new(strategy)).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    assert_eq!(
        engine.open_orders().next().unwrap().quantity,
        Money::from_f64(5.0)
    );
    engine.on_bar(&input, time(1)).unwrap();
    let position = engine
        .positions()
        .get_position(&Symbol::new("TEST"))
        .unwrap();
    assert!(position.risk_amount.to_f64() <= engine.equity() * 0.05 + 1e-8);
    assert!(position.quantity.to_f64() * 150.0 <= engine.equity() * 0.5 + 1e-8);
}

#[test]
fn limit_admission_reserves_the_actual_limit_price_without_slippage() {
    let mut cfg = zero_cost_config(false);
    cfg.trading.risk_per_trade = 1.0;
    cfg.exchange.assumed_slippage = 0.1;
    let strategy = ScriptedOrders::new(vec![vec![OrderRequest::limit_buy(
        Symbol::new("TEST"),
        10.0,
        100.0,
    )
    .with_quantity_cap()]]);
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 101.0, 99.0, 100.0),
    ]);
    let mut engine = TradingEngine::new(cfg, Box::new(strategy)).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    assert_eq!(engine.open_orders().count(), 1);
    engine.on_bar(&input, time(1)).unwrap();
    assert_eq!(
        engine
            .positions()
            .get_position(&Symbol::new("TEST"))
            .unwrap()
            .quantity,
        Money::from_f64(10.0)
    );
    assert_eq!(engine.cash(), 0.0);
}

#[test]
fn protective_stop_precedes_ambiguous_profit_reductions_but_not_open_fills() {
    for (open, expected_trades, expected_pnl) in [(100.0, 1, -20.0), (115.0, 2, 0.0)] {
        let strategy = ScriptedOrders::new(vec![
            vec![OrderRequest::market_buy(Symbol::new("TEST"), 2.0).with_quantity_cap()],
            vec![OrderRequest::limit_sell(Symbol::new("TEST"), 1.0, 110.0)],
        ]);
        let input = data(vec![
            candle(0, 100.0, 100.0, 100.0, 100.0),
            candle(1, 100.0, 100.0, 100.0, 100.0),
            candle(2, open, 120.0, 80.0, 85.0),
        ]);
        let mut engine = TradingEngine::new(zero_cost_config(false), Box::new(strategy)).unwrap();
        for hour in 0..3 {
            engine.on_bar(&input, time(hour)).unwrap();
        }
        assert_eq!(engine.positions().open_position_count(), 0);
        assert_eq!(engine.trades().len(), expected_trades);
        assert_eq!(
            engine
                .trades()
                .iter()
                .map(|trade| trade.net_pnl)
                .sum::<Money>(),
            Money::from_f64(expected_pnl)
        );
    }
}

#[test]
fn opposing_pending_entries_are_rejected_without_reversing_the_position() {
    let mut strategy = ScriptedOrders::new(vec![vec![
        OrderRequest::stop_buy(Symbol::new("TEST"), 2.0, 103.0).with_quantity_cap(),
        OrderRequest::stop_sell(Symbol::new("TEST"), 5.0, 97.0).with_quantity_cap(),
    ]]);
    strategy.target = 110.0;
    let mut engine = TradingEngine::new(zero_cost_config(true), Box::new(strategy)).unwrap();
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 104.0, 96.0, 100.0),
    ]);
    engine.on_bar(&input, time(0)).unwrap();
    assert_eq!(engine.open_orders().count(), 1);
    engine.on_bar(&input, time(1)).unwrap();
    assert_eq!(
        engine
            .positions()
            .get_position(&Symbol::new("TEST"))
            .unwrap()
            .side,
        Side::Buy
    );
    assert!(engine.trades().is_empty());
}

#[test]
fn final_cumulative_reports_are_idempotent_across_restart_and_reject_conflicts() {
    for restart in [false, true] {
        let cfg = zero_cost_config(true);
        let mut engine =
            TradingEngine::new(cfg.clone(), Box::new(Probe::new(Entry::Market))).unwrap();
        engine
            .on_bar(&data(vec![candle(0, 100.0, 100.0, 100.0, 100.0)]), time(0))
            .unwrap();
        let id = engine.open_orders().next().unwrap().id;
        engine
            .apply_cumulative_fill(cumulative(id, 2.0, 100.0, 1.0))
            .unwrap();
        if restart {
            let snapshot = serde_json::to_vec(&engine.snapshot().unwrap()).unwrap();
            engine = TradingEngine::new(cfg, Box::new(Probe::new(Entry::Market))).unwrap();
            engine
                .restore(serde_json::from_slice(&snapshot).unwrap())
                .unwrap();
        }
        let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
        engine
            .apply_cumulative_fill(cumulative(id, 2.0, 100.0, 1.0))
            .unwrap();
        assert_eq!(
            serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
            before
        );
        assert!(engine
            .apply_cumulative_fill(cumulative(id, 2.0, 101.0, 1.0))
            .is_err());
        assert!(engine
            .apply_cumulative_fill(cumulative(id + 1_000_000, 2.0, 100.0, 1.0))
            .is_err());
    }
}

fn exit_fixture(exit_quantity: f64, cancel_at: Option<usize>) -> (TradingEngine, ScriptedOrders) {
    let mut strategy = ScriptedOrders::new(vec![
        vec![OrderRequest::market_buy(Symbol::new("TEST"), 2.0).with_quantity_cap()],
        vec![OrderRequest::limit_sell(
            Symbol::new("TEST"),
            exit_quantity,
            110.0,
        )],
    ]);
    strategy.cancel_at = cancel_at;
    let mut engine =
        TradingEngine::new(zero_cost_config(true), Box::new(strategy.clone())).unwrap();
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 100.0, 100.0, 100.0),
    ]);
    engine.on_bar(&input, time(0)).unwrap();
    let id = engine.open_orders().next().unwrap().id;
    engine
        .apply_cumulative_fill(cumulative(id, 2.0, 100.0, 2.0))
        .unwrap();
    engine.on_bar(&input, time(1)).unwrap();
    (engine, strategy)
}

#[test]
fn exit_report_fragmentation_preserves_trade_fees_risk_and_restart() {
    for restart in [false, true] {
        let (mut engine, strategy) = exit_fixture(2.0, None);
        let id = engine.open_orders().next().unwrap().id;
        engine
            .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 1.0))
            .unwrap();
        assert!(engine.trades().is_empty());
        assert_eq!(engine.risk().consecutive_losses, 0);
        if restart {
            let snapshot = serde_json::to_vec(&engine.snapshot().unwrap()).unwrap();
            engine =
                TradingEngine::new(zero_cost_config(true), Box::new(strategy.clone())).unwrap();
            engine
                .restore(serde_json::from_slice(&snapshot).unwrap())
                .unwrap();
        }
        engine
            .apply_cumulative_fill(cumulative(id, 2.0, 99.0, 1.0))
            .unwrap();
        let (mut single, _) = exit_fixture(2.0, None);
        let single_id = single.open_orders().next().unwrap().id;
        single
            .apply_cumulative_fill(cumulative(single_id, 2.0, 99.0, 2.0))
            .unwrap();
        assert_eq!(
            serde_json::to_value(engine.trades()).unwrap(),
            serde_json::to_value(single.trades()).unwrap()
        );
        assert_eq!(engine.cash(), single.cash());
        assert_eq!(engine.risk().consecutive_losses, 1);
        assert_eq!(engine.risk().consecutive_wins, 0);
        assert_eq!(
            strategy.closed.lock().unwrap()[0].net_pnl,
            Money::from_f64(-6.0)
        );
    }
}

#[test]
fn cancelled_partial_exit_is_finalized_once_without_closing_the_position() {
    let (mut engine, strategy) = exit_fixture(2.0, Some(3));
    let id = engine.open_orders().next().unwrap().id;
    engine
        .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 1.0))
        .unwrap();
    let input = data(
        (0..4)
            .map(|hour| candle(hour, 100.0, 100.0, 100.0, 100.0))
            .collect(),
    );
    engine.on_bar(&input, time(2)).unwrap();
    engine.on_bar(&input, time(3)).unwrap();
    assert_eq!(engine.trades().len(), 1);
    assert_eq!(engine.trades()[0].quantity, Money::ONE);
    assert_eq!(engine.trades()[0].net_pnl, Money::from_f64(-12.0));
    assert_eq!(engine.risk().consecutive_losses, 1);
    assert_eq!(engine.positions().open_position_count(), 1);
    assert!(strategy.closed.lock().unwrap().is_empty());
}

#[test]
fn a_completed_grid_reduction_remains_a_trade_without_flattening_inventory() {
    let (mut engine, strategy) = exit_fixture(1.0, None);
    let id = engine.open_orders().next().unwrap().id;
    engine
        .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 1.0))
        .unwrap();
    assert_eq!(engine.trades().len(), 1);
    assert_eq!(engine.trades()[0].net_pnl, Money::from_f64(-12.0));
    assert_eq!(engine.risk().consecutive_losses, 1);
    assert_eq!(engine.positions().open_position_count(), 1);
    assert!(strategy.closed.lock().unwrap().is_empty());
}

#[test]
fn active_and_cancelled_execution_cursors_are_validated_and_persisted() {
    let (mut engine, strategy) = exit_fixture(2.0, Some(3));
    let id = engine.open_orders().next().unwrap().id;
    engine
        .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 1.0))
        .unwrap();
    let before = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
    engine
        .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 1.0))
        .unwrap();
    assert_eq!(
        serde_json::to_value(engine.snapshot().unwrap()).unwrap(),
        before
    );
    assert!(engine
        .apply_cumulative_fill(cumulative(id, 1.0, 91.0, 1.0))
        .is_err());
    assert!(engine
        .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 2.0))
        .is_err());
    let mut invalid = before;
    invalid["orders"][id.to_string()]["last_cumulative_fill"]["average_price"] =
        serde_json::json!("91");
    assert!(engine
        .restore(serde_json::from_value(invalid).unwrap())
        .is_err());
    let input = data(
        (0..3)
            .map(|hour| candle(hour, 100.0, 100.0, 100.0, 100.0))
            .collect(),
    );
    engine.on_bar(&input, time(2)).unwrap();
    let snapshot = serde_json::to_vec(&engine.snapshot().unwrap()).unwrap();
    engine = TradingEngine::new(zero_cost_config(true), Box::new(strategy)).unwrap();
    engine
        .restore(serde_json::from_slice(&snapshot).unwrap())
        .unwrap();
    engine
        .apply_cumulative_fill(cumulative(id, 1.0, 90.0, 1.0))
        .unwrap();
    assert_eq!(engine.trades().len(), 1);
    assert_eq!(engine.risk().consecutive_losses, 1);
    assert!(engine
        .apply_cumulative_fill(cumulative(id, 1.0, 91.0, 1.0))
        .is_err());
}

#[test]
fn terminal_cursor_eviction_uses_completion_order_not_creation_order() {
    let cfg = zero_cost_config(true);
    let mut engine = TradingEngine::new(cfg.clone(), Box::new(Probe::new(Entry::Market))).unwrap();
    engine
        .on_bar(&data(vec![candle(0, 100.0, 100.0, 100.0, 100.0)]), time(0))
        .unwrap();
    let old_id = engine.open_orders().next().unwrap().id;
    let mut snapshot = serde_json::to_value(engine.snapshot().unwrap()).unwrap();
    for id in (old_id + 1)..=(old_id + 4096) {
        snapshot["completed_fills"][id.to_string()] =
            serde_json::to_value(cumulative(id, 1.0, 100.0, 0.0)).unwrap();
    }
    snapshot["completion_order"] =
        serde_json::json!(((old_id + 1)..=(old_id + 4096)).collect::<Vec<_>>());
    snapshot["last_order_id"] = serde_json::json!(old_id + 4096);
    engine
        .restore(serde_json::from_value(snapshot).unwrap())
        .unwrap();
    engine
        .apply_cumulative_fill(cumulative(old_id, 2.0, 100.0, 0.0))
        .unwrap();
    let snapshot = engine.snapshot().unwrap();
    let json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(json["completed_fills"].as_object().unwrap().len(), 4096);
    assert_eq!(
        json["completion_order"].as_array().unwrap().last().unwrap(),
        old_id
    );
    assert!(json["completed_fills"]
        .get((old_id + 1).to_string())
        .is_none());
    engine = TradingEngine::new(cfg, Box::new(Probe::new(Entry::Market))).unwrap();
    engine.restore(snapshot).unwrap();
    engine
        .apply_cumulative_fill(cumulative(old_id, 2.0, 100.0, 0.0))
        .unwrap();
    assert!(engine
        .apply_cumulative_fill(cumulative(old_id + 1, 1.0, 100.0, 0.0))
        .is_err());
    let mut invalid = json;
    invalid["completion_order"].as_array_mut().unwrap().pop();
    assert!(engine
        .restore(serde_json::from_value(invalid).unwrap())
        .is_err());
}

#[test]
fn intrabar_entry_cannot_take_profit_at_a_pre_entry_price() {
    for side in [Side::Buy, Side::Sell] {
        let mut strategy = ScriptedOrders::new(vec![vec![match side {
            Side::Buy => {
                OrderRequest::limit_buy(Symbol::new("TEST"), 1.0, 90.0).with_quantity_cap()
            }
            Side::Sell => {
                OrderRequest::limit_sell(Symbol::new("TEST"), 1.0, 110.0).with_quantity_cap()
            }
        }]]);
        strategy.stop = if side == Side::Buy { 80.0 } else { 90.0 };
        strategy.target = if side == Side::Buy { 110.0 } else { 120.0 };
        let next = match side {
            Side::Buy => candle(1, 120.0, 120.0, 90.0, 100.0),
            Side::Sell => candle(1, 80.0, 110.0, 80.0, 100.0),
        };
        let input = data(vec![candle(0, 100.0, 100.0, 100.0, 100.0), next]);
        let mut engine = TradingEngine::new(zero_cost_config(false), Box::new(strategy)).unwrap();
        engine.on_bar(&input, time(0)).unwrap();
        engine.on_bar(&input, time(1)).unwrap();
        assert!(engine.trades().is_empty());
        assert_eq!(engine.equity(), 1010.0);
    }
}

#[test]
fn intrabar_fill_does_not_create_a_fictitious_peak_or_halt() {
    let mut strategy = ScriptedOrders::new(vec![vec![OrderRequest::limit_buy(
        Symbol::new("TEST"),
        1.0,
        90.0,
    )
    .with_quantity_cap()]]);
    strategy.stop = 80.0;
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 100.0, 90.0, 90.0),
    ]);
    let mut cfg = zero_cost_config(false);
    cfg.trading.max_drawdown = 0.005;
    cfg.trading.drawdown_warning = 0.003;
    cfg.trading.drawdown_critical = 0.004;
    let mut engine = TradingEngine::new(cfg, Box::new(strategy)).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    engine.on_bar(&input, time(1)).unwrap();
    assert_eq!(engine.equity(), 1000.0);
    assert_eq!(engine.risk().peak_capital(), 1000.0);
    assert!(!engine.risk().should_halt_trading());
}

#[test]
fn protection_precedes_close_marks_and_opening_exits_precede_intrabar_entries() {
    for (next, target, exit_price, expected_peak) in [
        (candle(2, 100.0, 120.0, 80.0, 115.0), 1000.0, 90.0, 1000.0),
        (candle(2, 120.0, 120.0, 95.0, 100.0), 110.0, 120.0, 1020.0),
    ] {
        let mut strategy = ScriptedOrders::new(vec![
            vec![OrderRequest::market_buy(Symbol::new("TEST"), 1.0).with_quantity_cap()],
            vec![OrderRequest::limit_buy(Symbol::new("TEST"), 1.0, 95.0).with_quantity_cap()],
        ]);
        strategy.target = target;
        let input = data(vec![
            candle(0, 100.0, 100.0, 100.0, 100.0),
            candle(1, 100.0, 100.0, 100.0, 100.0),
            next,
        ]);
        let mut engine = TradingEngine::new(zero_cost_config(false), Box::new(strategy)).unwrap();
        for hour in 0..3 {
            engine.on_bar(&input, time(hour)).unwrap();
        }
        assert_eq!(
            engine.trades().last().unwrap().exit_price,
            Money::from_f64(exit_price)
        );
        assert_eq!(engine.risk().peak_capital(), expected_peak);
        if target == 110.0 {
            assert_eq!(engine.trades().last().unwrap().quantity, Money::ONE);
        }
    }
}

#[test]
fn opening_fills_precede_older_intrabar_orders() {
    let strategy = ScriptedOrders::new(vec![
        vec![OrderRequest::market_buy(Symbol::new("TEST"), 2.0).with_quantity_cap()],
        vec![
            OrderRequest::limit_buy(Symbol::new("TEST"), 1.0, 95.0).with_quantity_cap(),
            OrderRequest::limit_sell(Symbol::new("TEST"), 1.0, 110.0),
        ],
    ]);
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 100.0, 100.0, 100.0),
        candle(2, 115.0, 115.0, 95.0, 95.0),
    ]);
    let mut cfg = zero_cost_config(false);
    cfg.trading.max_drawdown = 0.03;
    cfg.trading.drawdown_warning = 0.025;
    cfg.trading.drawdown_critical = 0.028;
    let mut engine = TradingEngine::new(cfg, Box::new(strategy)).unwrap();
    for hour in 0..3 {
        engine.on_bar(&input, time(hour)).unwrap();
    }
    assert_eq!(
        engine
            .positions()
            .get_position(&Symbol::new("TEST"))
            .unwrap()
            .quantity,
        Money::from_f64(2.0)
    );
    assert!(!engine.risk().should_halt_trading());
}

#[test]
fn intrabar_limit_crossings_follow_price_instead_of_creation_ids() {
    let mut strategy = ScriptedOrders::new(vec![vec![
        OrderRequest::limit_buy(Symbol::new("TEST"), 1.0, 80.0).with_quantity_cap(),
        OrderRequest::limit_buy(Symbol::new("TEST"), 1.0, 95.0).with_quantity_cap(),
    ]]);
    strategy.stop = 70.0;
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 100.0, 80.0, 80.0),
    ]);
    let mut engine = TradingEngine::new(zero_cost_config(false), Box::new(strategy)).unwrap();
    engine.on_bar(&input, time(0)).unwrap();
    engine.on_bar(&input, time(1)).unwrap();
    let position = engine
        .positions()
        .get_position(&Symbol::new("TEST"))
        .unwrap();
    assert_eq!(position.fills[0].price, Money::from_f64(95.0));
    assert_eq!(position.fills[1].price, Money::from_f64(80.0));
    assert_eq!(engine.risk().peak_capital(), 1000.0);
    assert_eq!(engine.equity(), 985.0);
}

#[test]
fn a_closer_resting_stop_reduces_before_the_protective_stop() {
    let strategy = ScriptedOrders::new(vec![
        vec![OrderRequest::market_buy(Symbol::new("TEST"), 2.0).with_quantity_cap()],
        vec![OrderRequest::stop_sell(Symbol::new("TEST"), 1.0, 95.0)],
    ]);
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 100.0, 100.0, 100.0),
        candle(2, 100.0, 100.0, 80.0, 85.0),
    ]);
    let mut engine = TradingEngine::new(zero_cost_config(false), Box::new(strategy)).unwrap();
    for hour in 0..3 {
        engine.on_bar(&input, time(hour)).unwrap();
    }
    assert_eq!(engine.trades().len(), 2);
    assert_eq!(engine.trades()[0].exit_price, Money::from_f64(95.0));
    assert_eq!(engine.trades()[1].exit_price, Money::from_f64(90.0));
    assert_eq!(engine.equity(), 985.0);
}

#[test]
fn opening_reductions_release_exposure_before_earlier_queued_additions() {
    let strategy = ScriptedOrders::new(vec![
        vec![OrderRequest::market_buy(Symbol::new("TEST"), 1.0).with_quantity_cap()],
        vec![
            OrderRequest::market_buy(Symbol::new("TEST"), 1.0).with_quantity_cap(),
            OrderRequest::market_sell(Symbol::new("TEST"), 1.0),
        ],
    ]);
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(1, 100.0, 100.0, 100.0, 100.0),
        candle(2, 200.0, 200.0, 200.0, 200.0),
    ]);
    let mut cfg = zero_cost_config(true);
    cfg.trading.max_position_pct = 0.2;
    let mut engine = TradingEngine::new(cfg, Box::new(strategy)).unwrap();
    for hour in 0..3 {
        engine.on_bar(&input, time(hour)).unwrap();
    }
    let position = engine
        .positions()
        .get_position(&Symbol::new("TEST"))
        .unwrap();
    assert_eq!(position.quantity, Money::ONE);
    assert_eq!(position.average_entry_price, Money::from_f64(200.0));
}
#[test]
fn insolvent_backtests_keep_the_debt_and_mark_calmar_as_undefined() {
    let input = data(vec![
        candle(0, 100.0, 100.0, 100.0, 100.0),
        candle(24, 100.0, 100.0, 100.0, 100.0),
        candle(48, 10000.0, 10000.0, 10000.0, 10000.0),
        candle(72, 10000.0, 10000.0, 10000.0, 10000.0),
    ]);
    let result = Backtester::new(zero_cost_config(false), Box::new(Probe::new(Entry::Short)))
        .run(&input)
        .unwrap();
    assert_eq!(result.metrics.total_return, -550.0);
    assert_eq!(result.metrics.calmar_ratio, f64::NEG_INFINITY);
    assert!(result.equity_curve.last().unwrap().1 < 0.0);
}
