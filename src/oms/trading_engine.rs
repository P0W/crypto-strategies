//! Deterministic bar-event processing shared by historical and streaming paper adapters.

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use super::{
    evaluate_exit, reserve_order_id, size_order, tighten_trailing_stop, ExecutionEngine, Fill,
    Order, OrderRequest, OrderState, OrderType, PositionManager, SizedOrder, StrategyContext,
};
use crate::data::candle_close_time;
use crate::multi_timeframe::{MultiSymbolMultiTimeframeData, MultiTimeframeCandles};
use crate::risk::RiskManager;
use crate::strategies::close_position_order;
use crate::{Candle, Config, Money, Side, Strategy, Symbol, Trade, PNL_EPSILON};

const LOOKBACK: usize = 300;
const COMPLETED_FILL_CAPACITY: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FillPhase {
    OpeningReductions,
    OpeningEntries,
    Intrabar,
}

#[derive(Clone, Serialize, Deserialize)]
struct WorkingOrder {
    order: Order,
    levels: Option<(f64, f64)>,
    reduce_only: bool,
    regime_score: f64,
    realized: Option<Trade>,
    last_cumulative_fill: Option<CumulativeFill>,
    #[serde(default)]
    protective: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum ExecutionMode {
    #[default]
    Simulated,
    ExternalSpot,
}

#[derive(Clone, Serialize, Deserialize)]
struct FeeBasis {
    symbol: Symbol,
    reduce_only: bool,
    quantity: Money,
    disposed: BTreeMap<u64, Money>,
    trade_index: Option<usize>,
}

/// An execution adapter may change an unsent order only through engine methods.
#[derive(Clone)]
pub struct ExecutionIntent {
    pub order: Order,
    pub reduce_only: bool,
    pub protective: bool,
    pub cancel_requested: bool,
    pub levels: Option<(f64, f64)>,
}

impl WorkingOrder {
    fn accumulate(&mut self, trade: Trade) {
        if let Some(total) = &mut self.realized {
            let quantity = total.quantity + trade.quantity;
            total.entry_price = (total.entry_price * total.quantity
                + trade.entry_price * trade.quantity)
                / quantity;
            total.exit_price =
                (total.exit_price * total.quantity + trade.exit_price * trade.quantity) / quantity;
            total.quantity = quantity;
            total.entry_time = total.entry_time.min(trade.entry_time);
            total.exit_time = total.exit_time.max(trade.exit_time);
            total.pnl += trade.pnl;
            total.commission += trade.commission;
            total.net_pnl += trade.net_pnl;
        } else {
            self.realized = Some(trade);
        }
    }
}

/// One atomic persistence unit, including FIFO lots and pending-order accounting.
#[derive(Clone, Serialize, Deserialize)]
pub struct EngineSnapshot {
    #[serde(default)]
    mode: ExecutionMode,
    #[serde(default)]
    cancel_requested: BTreeSet<u64>,
    #[serde(default)]
    fee_basis: BTreeMap<u64, FeeBasis>,
    cash: f64,
    risk: RiskManager,
    positions: PositionManager,
    orders: BTreeMap<u64, WorkingOrder>,
    completed_fills: BTreeMap<u64, CumulativeFill>,
    completion_order: VecDeque<u64>,
    levels: HashMap<Symbol, (f64, f64)>,
    trailing: HashMap<Symbol, f64>,
    prices: HashMap<Symbol, f64>,
    trades: Vec<Trade>,
    last_bar: Option<DateTime<Utc>>,
    last_order_id: u64,
    strategy_state: serde_json::Value,
    cost_state: super::costs::CostState,
}

pub struct TradingEngine {
    config: Config,
    strategy: Box<dyn Strategy>,
    execution: ExecutionEngine,
    state: EngineSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CumulativeFill {
    pub order_id: u64,
    pub quantity: Money,
    pub average_price: Money,
    pub incremental_commission: Money,
    pub is_maker: bool,
    pub timestamp: DateTime<Utc>,
}

impl TradingEngine {
    pub fn new(config: Config, mut strategy: Box<dyn Strategy>) -> Result<Self> {
        let t = &config.trading;
        ensure!(
            t.initial_capital.is_finite() && t.initial_capital > 0.0,
            "Initial capital must be finite and positive"
        );
        for value in [
            t.risk_per_trade,
            t.max_portfolio_heat,
            t.max_position_pct,
            t.max_drawdown,
        ] {
            ensure!(
                value.is_finite() && value > 0.0 && value <= 1.0,
                "Invalid risk fraction"
            );
        }
        ensure!(t.max_positions > 0, "Position limit must be positive");
        ensure!(
            t.drawdown_warning.is_finite()
                && t.drawdown_critical.is_finite()
                && t.drawdown_warning >= 0.0
                && t.drawdown_warning <= t.drawdown_critical
                && t.drawdown_critical <= t.max_drawdown,
            "Invalid drawdown thresholds"
        );
        for multiplier in [
            t.drawdown_warning_multiplier,
            t.drawdown_critical_multiplier,
            t.consecutive_loss_multiplier,
        ] {
            ensure!(
                multiplier.is_finite() && (0.0..=1.0).contains(&multiplier),
                "Invalid risk multiplier"
            );
        }
        ensure!(
            config.exchange.assumed_slippage.is_finite()
                && (0.0..1.0).contains(&config.exchange.assumed_slippage),
            "Invalid slippage"
        );
        for fee in [config.exchange.maker_fee, config.exchange.taker_fee] {
            ensure!(
                fee.is_finite() && (0.0..=1.0).contains(&fee),
                "Invalid commission rate"
            );
        }
        if let crate::config::TransactionCostConfig::Components {
            brokerage_rate,
            brokerage_cap_per_order,
            buy_turnover_rate,
            sell_turnover_rate,
            exchange_rate,
            regulatory_rate,
            buy_stamp_rate,
            indirect_tax_rate,
            sell_fixed_charge,
            ..
        } = &config.exchange.cost_model
        {
            ensure!(
                [
                    brokerage_rate,
                    brokerage_cap_per_order,
                    buy_turnover_rate,
                    sell_turnover_rate,
                    exchange_rate,
                    regulatory_rate,
                    buy_stamp_rate,
                    indirect_tax_rate,
                    sell_fixed_charge
                ]
                .iter()
                .all(|value| value.is_finite() && **value >= 0.0),
                "Transaction costs must be finite and nonnegative"
            );
        }
        let risk = RiskManager::new(
            t.initial_capital,
            t.risk_per_trade,
            t.max_positions,
            t.max_portfolio_heat,
            t.max_position_pct,
            t.max_drawdown,
            t.drawdown_warning,
            t.drawdown_critical,
            t.drawdown_warning_multiplier,
            t.drawdown_critical_multiplier,
            t.consecutive_loss_limit,
            t.consecutive_loss_multiplier,
        );
        strategy.init();
        Ok(Self {
            execution: ExecutionEngine::from_exchange_config(&config.exchange),
            state: EngineSnapshot {
                mode: ExecutionMode::Simulated,
                cancel_requested: BTreeSet::new(),
                fee_basis: BTreeMap::new(),
                cash: t.initial_capital,
                risk,
                positions: PositionManager::new(),
                orders: BTreeMap::new(),
                completed_fills: BTreeMap::new(),
                completion_order: VecDeque::new(),
                levels: HashMap::new(),
                trailing: HashMap::new(),
                prices: HashMap::new(),
                trades: Vec::new(),
                last_bar: None,
                last_order_id: 0,
                strategy_state: serde_json::Value::Null,
                cost_state: super::costs::CostState::default(),
            },
            config,
            strategy,
        })
    }

    pub fn new_external_spot(config: Config, strategy: Box<dyn Strategy>) -> Result<Self> {
        let mut engine = Self::new(config, strategy)?;
        engine.state.mode = ExecutionMode::ExternalSpot;
        Ok(engine)
    }

    pub fn equity(&self) -> f64 {
        self.state.cash
            + self
                .state
                .positions
                .get_all_positions()
                .map(|(symbol, pos)| pos.equity_contribution(self.state.prices[symbol]))
                .sum::<f64>()
    }

    pub fn cash(&self) -> f64 {
        self.state.cash
    }
    pub fn trades(&self) -> &[Trade] {
        &self.state.trades
    }
    pub fn risk(&self) -> &RiskManager {
        &self.state.risk
    }
    pub fn positions(&self) -> &PositionManager {
        &self.state.positions
    }
    pub fn last_bar(&self) -> Option<DateTime<Utc>> {
        self.state.last_bar
    }
    pub fn open_orders(&self) -> impl Iterator<Item = &Order> {
        self.state.orders.values().map(|working| &working.order)
    }

    pub fn snapshot(&self) -> Result<EngineSnapshot> {
        let mut snapshot = self.state.clone();
        snapshot.cost_state = self.execution.cost_state();
        snapshot.strategy_state = self.strategy.snapshot_state()?;
        Ok(snapshot)
    }

    pub fn restore(&mut self, snapshot: EngineSnapshot) -> Result<()> {
        ensure!(
            snapshot.mode == self.state.mode,
            "Execution mode/snapshot mismatch"
        );
        ensure!(
            snapshot
                .cancel_requested
                .iter()
                .all(|id| snapshot.orders.contains_key(id)),
            "Cancellation references a missing order"
        );
        ensure!(snapshot.cash.is_finite(), "Invalid snapshot cash");
        for (symbol, pos) in snapshot.positions.get_all_positions() {
            ensure!(
                snapshot
                    .prices
                    .get(symbol)
                    .is_some_and(|p| p.is_finite() && *p > 0.0),
                "Missing or invalid price for restored position {symbol}"
            );
            ensure!(
                snapshot.levels.contains_key(symbol),
                "Missing stop/target for {symbol}"
            );
            let quantity: Money = pos.fills.iter().map(|fill| fill.quantity).sum();
            ensure!(
                quantity == pos.quantity,
                "Inconsistent FIFO lots for {symbol}"
            );
        }
        for (id, working) in &snapshot.orders {
            ensure!(
                *id == working.order.id && working.order.is_active(),
                "Invalid restored order"
            );
            reserve_order_id(*id);
            ensure!(
                snapshot
                    .prices
                    .get(&working.order.symbol)
                    .is_some_and(|p| p.is_finite() && *p > 0.0),
                "Missing order valuation"
            );
            ensure!(
                working.order.quantity.is_positive()
                    && working.order.filled_quantity >= Money::ZERO
                    && working.order.remaining_quantity.is_positive()
                    && working.order.remaining_quantity + working.order.filled_quantity
                        == working.order.quantity,
                "Inconsistent restored order quantities"
            );
            ensure!(
                working.regime_score.is_finite() && working.regime_score >= 0.0,
                "Invalid restored regime score"
            );
            if working.order.filled_quantity.is_positive() {
                let report = working
                    .last_cumulative_fill
                    .as_ref()
                    .context("Missing active execution cursor")?;
                ensure!(
                    report.order_id == *id
                        && report.quantity == working.order.filled_quantity
                        && report.average_price == working.order.average_fill_price
                        && report.incremental_commission >= Money::ZERO,
                    "Inconsistent active execution cursor"
                );
            } else {
                ensure!(
                    working.last_cumulative_fill.is_none(),
                    "Unexpected active execution cursor"
                );
            }
            if working.reduce_only && working.order.filled_quantity.is_positive() {
                let trade = working
                    .realized
                    .as_ref()
                    .context("Missing partial-exit accounting")?;
                ensure!(
                    trade.quantity == working.order.filled_quantity
                        && trade.symbol == working.order.symbol
                        && trade.side != working.order.side
                        && trade.commission >= Money::ZERO
                        && trade.net_pnl == trade.pnl - trade.commission,
                    "Inconsistent partial-exit accounting"
                );
            } else {
                ensure!(
                    working.realized.is_none(),
                    "Unexpected partial-exit accounting"
                );
            }
        }
        ensure!(
            snapshot.completed_fills.len() <= COMPLETED_FILL_CAPACITY,
            "Oversized execution cursor cache"
        );
        let completion_ids: HashSet<_> = snapshot.completion_order.iter().copied().collect();
        ensure!(
            snapshot.completion_order.len() == snapshot.completed_fills.len()
                && completion_ids.len() == snapshot.completion_order.len()
                && snapshot
                    .completed_fills
                    .keys()
                    .all(|id| completion_ids.contains(id)),
            "Inconsistent terminal execution cursor order"
        );
        for (id, report) in &snapshot.completed_fills {
            ensure!(
                *id == report.order_id
                    && !snapshot.orders.contains_key(id)
                    && report.quantity.is_positive()
                    && report.average_price.is_positive()
                    && report.incremental_commission >= Money::ZERO,
                "Invalid completed execution cursor"
            );
            reserve_order_id(*id);
        }
        reserve_order_id(snapshot.last_order_id);
        self.strategy
            .restore_state(snapshot.strategy_state.clone())?;
        self.execution
            .restore_cost_state(snapshot.cost_state.clone());
        self.state = snapshot;
        self.refresh_risk();
        Ok(())
    }

    /// Process one synchronized, closed primary candle across every symbol.
    /// Duplicate events are ignored; out-of-order events fail explicitly.
    pub fn on_bar(
        &mut self,
        data: &MultiSymbolMultiTimeframeData,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        if let Some(last) = self.state.last_bar {
            ensure!(timestamp >= last, "Out-of-order candle event");
            if timestamp == last {
                return Ok(());
            }
        }
        ensure!(!data.is_empty(), "Empty market frame");
        ensure!(
            self.state
                .positions
                .get_all_positions()
                .all(|(symbol, _)| data.contains_key(symbol))
                && self
                    .state
                    .orders
                    .values()
                    .all(|w| data.contains_key(&w.order.symbol)),
            "Market frame omits a symbol with an open position or order"
        );
        let mut symbols: Vec<_> = data.keys().collect();
        symbols.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let primary_tf = data[symbols[0]].primary_timeframe();
        let frames: Vec<_> = symbols
            .iter()
            .map(|symbol| {
                let mtf = &data[*symbol];
                ensure!(
                    mtf.primary_timeframe() == primary_tf,
                    "Primary timeframes differ across symbols"
                );
                let primary_end = mtf.primary().partition_point(|c| c.datetime <= timestamp);
                let primary = &mtf.primary()[primary_end.saturating_sub(LOOKBACK)..primary_end];
                ensure!(
                    primary.last().is_some_and(|c| c.datetime == timestamp),
                    "Missing synchronized candle for {symbol} at {timestamp}"
                );
                let as_of = candle_close_time(timestamp, mtf.primary_timeframe())
                    .context("Unsupported primary timeframe")?;
                let mut view = MultiTimeframeCandles::new(mtf.primary_timeframe(), timestamp);
                view.add_timeframe(mtf.primary_timeframe(), primary);
                for tf in mtf.timeframes() {
                    if tf == mtf.primary_timeframe() {
                        continue;
                    }
                    ensure!(
                        candle_close_time(timestamp, tf).is_some(),
                        "Unsupported timeframe {tf}"
                    );
                    let candles = mtf.get(tf).context("Missing timeframe")?;
                    let end = candles.partition_point(|c| {
                        candle_close_time(c.datetime, tf).is_some_and(|close| close <= as_of)
                    });
                    view.add_timeframe(tf, &candles[end.saturating_sub(LOOKBACK)..end]);
                }
                Ok((*symbol, view))
            })
            .collect::<Result<_>>()?;

        if self.state.mode == ExecutionMode::ExternalSpot {
            for (symbol, view) in &frames {
                self.state.prices.insert(
                    (*symbol).clone(),
                    view.primary().last().context("Missing close")?.close,
                );
            }
            self.refresh_risk();
            for (symbol, view) in &frames {
                self.decide(symbol, view)?;
                self.refresh_risk();
            }
            self.state.last_bar = Some(timestamp);
            return Ok(());
        }

        for (symbol, view) in &frames {
            self.state
                .prices
                .insert((*symbol).clone(), view.primary().last().unwrap().open);
        }
        self.refresh_risk();
        let mut protected = HashSet::new();
        for phase in [FillPhase::OpeningReductions, FillPhase::OpeningEntries] {
            for (symbol, view) in &frames {
                if !protected.contains(symbol) {
                    self.fill_resting_orders(symbol, view.primary().last().unwrap(), phase)?;
                }
            }
            for (symbol, view) in &frames {
                let candle = view.primary().last().unwrap();
                let opening = Candle::new_unchecked(
                    candle.datetime,
                    candle.open,
                    candle.open,
                    candle.open,
                    candle.open,
                    candle.volume,
                );
                if !protected.contains(symbol) && self.apply_protection(symbol, &opening)? {
                    protected.insert(*symbol);
                }
            }
        }
        let mut protection_frames = Vec::new();
        for (symbol, view) in &frames {
            if protected.contains(symbol) {
                continue;
            }
            let candle = view.primary().last().unwrap();
            let mut remaining_bar = candle.clone();
            if let Some((side, price)) =
                self.fill_resting_orders(symbol, candle, FillPhase::Intrabar)?
            {
                // Favorable extremes may precede an intrabar entry; the close cannot.
                remaining_bar.open = price;
                match side {
                    Side::Buy => {
                        remaining_bar.high = price.max(candle.close);
                        remaining_bar.low = candle.low.min(price);
                    }
                    Side::Sell => {
                        remaining_bar.low = price.min(candle.close);
                        remaining_bar.high = candle.high.max(price);
                    }
                }
            }
            protection_frames.push((*symbol, remaining_bar));
        }
        for (symbol, candle) in &protection_frames {
            if self.apply_protection(symbol, candle)? {
                protected.insert(*symbol);
            }
        }
        for (symbol, view) in &frames {
            self.state
                .prices
                .insert((*symbol).clone(), view.primary().last().unwrap().close);
        }
        self.refresh_risk();
        for (symbol, view) in &frames {
            if !protected.contains(symbol) {
                self.decide(symbol, view)?;
            }
            self.refresh_risk();
        }
        self.state.last_bar = Some(timestamp);
        Ok(())
    }

    fn refresh_risk(&mut self) {
        self.state
            .positions
            .update_unrealized_pnl(&self.state.prices);
        for (symbol, (initial_stop, _)) in &self.state.levels {
            if let Some(pos) = self.state.positions.get_position_mut(symbol) {
                let stop = self
                    .state
                    .trailing
                    .get(symbol)
                    .copied()
                    .unwrap_or(*initial_stop);
                let distance = match pos.side {
                    Side::Buy => pos.average_entry_price.to_f64() - stop,
                    Side::Sell => stop - pos.average_entry_price.to_f64(),
                };
                pos.set_risk_amount(distance.max(0.0) * pos.quantity.to_f64());
            }
        }
        self.state.risk.update_capital(self.equity());
    }

    fn cancel(&mut self, id: u64) {
        if self.state.mode == ExecutionMode::ExternalSpot {
            if self.state.orders.contains_key(&id) {
                self.state.cancel_requested.insert(id);
            }
            return;
        }
        self.complete_cancel(id);
    }

    fn complete_cancel(&mut self, id: u64) {
        self.state.cancel_requested.remove(&id);
        if let Some(mut working) = self.state.orders.remove(&id) {
            working.order.state = OrderState::Cancelled;
            if let Some(trade) = working.realized.take() {
                if let Some(basis) = self.state.fee_basis.get_mut(&id) {
                    basis.trade_index = Some(self.state.trades.len());
                }
                self.record_trade(trade);
            }
            if let Some(report) = working.last_cumulative_fill.take() {
                self.remember_completed_fill(report);
            }
            self.strategy.on_order_cancelled(&working.order);
        }
    }

    fn record_trade(&mut self, trade: Trade) {
        if trade.net_pnl.to_f64() > PNL_EPSILON {
            self.state.risk.record_win();
        } else if trade.net_pnl.to_f64() < -PNL_EPSILON {
            self.state.risk.record_loss();
        }
        self.state.trades.push(trade);
    }

    fn remember_completed_fill(&mut self, report: CumulativeFill) {
        let id = report.order_id;
        if self.state.completed_fills.insert(id, report).is_none() {
            self.state.completion_order.push_back(id);
        }
        if self.state.completion_order.len() > COMPLETED_FILL_CAPACITY {
            if let Some(oldest) = self.state.completion_order.pop_front() {
                self.state.completed_fills.remove(&oldest);
            }
        }
    }

    fn fill_resting_orders(
        &mut self,
        symbol: &Symbol,
        candle: &Candle,
        phase: FillPhase,
    ) -> Result<Option<(Side, f64)>> {
        let open_phase = phase != FillPhase::Intrabar;
        let mut last_intrabar_entry = None;
        let opening = Candle::new_unchecked(
            candle.datetime,
            candle.open,
            candle.open,
            candle.open,
            candle.open,
            candle.volume,
        );
        let mut ids: Vec<_> = self
            .state
            .orders
            .values()
            .filter(|w| &w.order.symbol == symbol && w.order.created_at < candle.datetime)
            .map(|w| w.order.id)
            .collect();
        if !open_phase {
            let side = self
                .state
                .positions
                .get_position(symbol)
                .map(|position| position.side)
                .or_else(|| {
                    ids.iter().find_map(|id| {
                        let working = &self.state.orders[id];
                        (!working.reduce_only).then_some(working.order.side)
                    })
                });
            if let Some(side) = side {
                let trigger = |id: &u64| {
                    let order = &self.state.orders[id].order;
                    let down = matches!(
                        (order.side, order.order_type),
                        (Side::Buy, OrderType::Limit) | (Side::Sell, OrderType::Stop)
                    );
                    let price = order
                        .limit_price
                        .or(order.stop_price)
                        .map_or(candle.open, Money::to_f64);
                    (
                        down != (side == Side::Buy),
                        if down { -price } else { price },
                    )
                };
                // Active limits are crossed by price, not by the age of their order IDs.
                ids.sort_by(|a, b| {
                    let (leg_a, price_a) = trigger(a);
                    let (leg_b, price_b) = trigger(b);
                    leg_a
                        .cmp(&leg_b)
                        .then_with(|| price_a.total_cmp(&price_b))
                        .then_with(|| a.cmp(b))
                });
            }
        }
        for id in ids {
            let working = &self.state.orders[&id];
            let at_open = self
                .execution
                .check_fill(&working.order, &opening, None)
                .is_some();
            let wrong_kind = match phase {
                FillPhase::OpeningReductions => !working.reduce_only,
                FillPhase::OpeningEntries => working.reduce_only,
                FillPhase::Intrabar => false,
            };
            if at_open != open_phase || wrong_kind {
                continue;
            }
            if !working.reduce_only && self.state.risk.should_halt_trading() {
                self.cancel(id);
                continue;
            }
            if !at_open && working.reduce_only {
                if let Some(position) = self.state.positions.get_position(symbol) {
                    let (stop, target) = self.state.levels[symbol];
                    let stop = self.state.trailing.get(symbol).copied().unwrap_or(stop);
                    let stop_hit = evaluate_exit(position.side, candle, stop, target)
                        .is_some_and(|exit| exit.reason == super::execution::ExitReason::Stop);
                    let earlier_stop_order = working.order.order_type == OrderType::Stop
                        && working
                            .order
                            .stop_price
                            .is_some_and(|trigger| match position.side {
                                Side::Buy => trigger.to_f64() >= stop,
                                Side::Sell => trigger.to_f64() <= stop,
                            });
                    // OHLC cannot establish a profit fill before an intrabar protective stop.
                    if stop_hit && !earlier_stop_order {
                        continue;
                    }
                }
            }
            if let Some(price) = self.execution.check_fill(&working.order, candle, None) {
                let entry_side = (!working.reduce_only).then_some(working.order.side);
                let price_value = if working.order.order_type == OrderType::Market {
                    self.slipped(price.price, working.order.side)
                } else {
                    price.price
                };
                if !at_open {
                    self.state.prices.insert(symbol.clone(), price_value);
                    self.refresh_risk();
                }
                let filled =
                    self.simulate_fill(id, price_value, price.is_maker, candle.datetime)?;
                if filled && !at_open {
                    if let Some(side) = entry_side {
                        last_intrabar_entry = Some((side, price_value));
                    }
                }
            }
        }
        Ok(last_intrabar_entry)
    }

    fn apply_protection(&mut self, symbol: &Symbol, candle: &Candle) -> Result<bool> {
        if let Some(pos) = self.state.positions.get_position(symbol).cloned() {
            let (stop, target) = self.state.levels[symbol];
            let active_stop = self.state.trailing.get(symbol).copied().unwrap_or(stop);
            if let Some(trigger) = evaluate_exit(pos.side, candle, active_stop, target) {
                let ids: Vec<_> = self
                    .state
                    .orders
                    .values()
                    .filter(|w| &w.order.symbol == symbol)
                    .map(|w| w.order.id)
                    .collect();
                for id in ids {
                    self.cancel(id);
                }
                let request = close_position_order(symbol, &pos);
                let id = self.place(request.to_order(), None, true, candle.datetime, 1.0);
                if !self.config.backtest.use_t1_execution {
                    let price =
                        self.slipped(trigger.execution_price, self.state.orders[&id].order.side);
                    self.simulate_fill(id, price, false, candle.datetime)?;
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn decide(&mut self, symbol: &Symbol, view: &MultiTimeframeCandles<'_>) -> Result<()> {
        let candles = view.primary();
        let candle = candles.last().context("Missing primary candle")?;
        if let Some(pos) = self.state.positions.get_position(symbol).cloned() {
            let initial_stop = self.state.levels[symbol].0;
            let active_stop = self
                .state
                .trailing
                .get(symbol)
                .copied()
                .unwrap_or(initial_stop);
            if let Some(candidate) = self
                .strategy
                .update_trailing_stop(&pos, candle.close, candles)
            {
                ensure!(
                    candidate.is_finite() && candidate > 0.0,
                    "Invalid trailing stop"
                );
                self.state.trailing.insert(
                    symbol.clone(),
                    tighten_trailing_stop(pos.side, Some(active_stop), candidate),
                );
                self.refresh_risk();
            }
        }

        let mut open_orders: Vec<_> = self
            .state
            .orders
            .values()
            .filter(|w| &w.order.symbol == symbol && !w.protective)
            .map(|w| w.order.clone())
            .collect();
        let cancellation_ids = {
            let ctx = self.context(symbol, view, &open_orders);
            self.strategy.orders_to_cancel(&ctx)
        };
        for id in cancellation_ids {
            if self
                .state
                .orders
                .get(&id)
                .is_some_and(|w| &w.order.symbol == symbol)
            {
                self.cancel(id);
            }
        }
        open_orders.retain(|order| self.state.orders.contains_key(&order.id));
        let equity = self.equity();
        let available = self.available_cash();
        let ctx = StrategyContext::multi_timeframe(
            symbol,
            view,
            self.state.positions.get_position(symbol),
            &open_orders,
            available,
            equity,
        )
        .with_peak_equity(self.state.risk.peak_capital());
        self.strategy.on_bar(&ctx);
        let requests = self.strategy.generate_orders(&ctx);
        for request in requests {
            ensure!(
                &request.symbol == symbol,
                "Strategy emitted an order for a different symbol"
            );
            self.submit(request, candles)?;
        }
        Ok(())
    }

    fn context<'a>(
        &'a self,
        symbol: &'a Symbol,
        view: &'a MultiTimeframeCandles<'a>,
        orders: &'a [Order],
    ) -> StrategyContext<'a> {
        StrategyContext::multi_timeframe(
            symbol,
            view,
            self.state.positions.get_position(symbol),
            orders,
            self.available_cash(),
            self.equity(),
        )
        .with_peak_equity(self.state.risk.peak_capital())
    }

    fn available_cash(&self) -> f64 {
        self.available_cash_except(None)
    }

    fn available_cash_except(&self, exclude: Option<u64>) -> f64 {
        self.state.cash
            - self
                .state
                .orders
                .values()
                .filter(|w| w.order.side == Side::Buy && Some(w.order.id) != exclude)
                .map(|w| {
                    let price = w
                        .order
                        .limit_price
                        .or(w.order.stop_price)
                        .map(|p| p.to_f64())
                        .unwrap_or(self.state.prices[&w.order.symbol]);
                    let turnover = self.order_execution_price(&w.order, price)
                        * w.order.remaining_quantity.to_f64();
                    turnover
                        + self
                            .execution
                            .estimate_commission(
                                Side::Buy,
                                turnover,
                                w.order.order_type == OrderType::Limit,
                            )
                            .to_f64()
                })
                .sum::<f64>()
    }

    fn pending_heat(&self, exclude: Option<u64>) -> f64 {
        self.state
            .orders
            .values()
            .filter(|working| !working.reduce_only && Some(working.order.id) != exclude)
            .filter_map(|working| {
                working.levels.map(|(stop, _)| {
                    let price = working
                        .order
                        .limit_price
                        .or(working.order.stop_price)
                        .map_or(self.state.prices[&working.order.symbol], Money::to_f64);
                    (price - stop).abs() * working.order.remaining_quantity.to_f64()
                })
            })
            .sum()
    }

    fn committed_exposure(&self, symbol: &Symbol, price: f64, exclude: Option<u64>) -> f64 {
        self.state
            .positions
            .get_position(symbol)
            .map_or(0.0, |position| position.quantity.to_f64() * price)
            + self
                .state
                .orders
                .values()
                .filter(|working| {
                    !working.reduce_only
                        && &working.order.symbol == symbol
                        && Some(working.order.id) != exclude
                })
                .map(|working| {
                    working.order.remaining_quantity.to_f64()
                        * working
                            .order
                            .limit_price
                            .or(working.order.stop_price)
                            .map_or(price, Money::to_f64)
                })
                .sum::<f64>()
    }

    fn execution_entry_capacity(&self, id: u64, price: f64, capital: f64) -> Result<f64> {
        let working = &self.state.orders[&id];
        let order = &working.order;
        let existing = self.state.positions.get_position(&order.symbol);
        if capital <= 0.0
            || existing.is_some_and(|position| position.side != order.side)
            || (existing.is_none()
                && !self
                    .state
                    .risk
                    .can_open_position_count(self.state.positions.open_position_count()))
        {
            return Ok(0.0);
        }
        let (initial_stop, target) = working.levels.context("Entry is missing protection")?;
        let stop = self
            .state
            .trailing
            .get(&order.symbol)
            .copied()
            .unwrap_or(initial_stop);
        let valid_entry = match order.side {
            Side::Buy => stop < price && price < target,
            Side::Sell => target < price && price < stop,
        };
        if !valid_entry {
            return Ok(0.0);
        }
        let positions: Vec<_> = self
            .state
            .positions
            .get_all_positions()
            .map(|(_, position)| position)
            .collect();
        let mut risk = self.state.risk.clone();
        risk.current_capital = capital;
        risk.max_portfolio_heat =
            (capital * risk.max_portfolio_heat - self.pending_heat(Some(id))).max(0.0) / capital;
        let quantity =
            risk.calculate_position_size_with_regime(price, stop, &positions, working.regime_score);
        let valuation = price.max(self.state.prices[&order.symbol]);
        let exposure_left = (capital * risk.max_position_pct
            - self.committed_exposure(&order.symbol, valuation, Some(id)))
        .max(0.0);
        Ok(quantity
            .min(exposure_left / valuation)
            .min(order.remaining_quantity.to_f64()))
    }

    fn submit(&mut self, request: OrderRequest, candles: &[Candle]) -> Result<()> {
        let candle = candles.last().context("Missing sizing candle")?;
        let is_exit = self
            .state
            .positions
            .get_position(&request.symbol)
            .is_some_and(|position| position.side != request.side);
        if self.state.mode == ExecutionMode::ExternalSpot && !is_exit && request.side == Side::Sell
        {
            tracing::warn!(symbol = %request.symbol, "Spot entry rejected: short selling is not supported");
            return Ok(());
        }
        if !is_exit
            && self.state.orders.values().any(|working| {
                !working.reduce_only
                    && working.order.symbol == request.symbol
                    && working.order.side != request.side
            })
        {
            tracing::warn!(symbol = %request.symbol, "Order rejected: opposing pending entry requires an explicit OCO policy");
            return Ok(());
        }
        let regime_score = self.strategy.get_regime_score(candles);
        ensure!(
            regime_score.is_finite() && regime_score >= 0.0,
            "Invalid regime score"
        );
        let positions: Vec<_> = self
            .state
            .positions
            .get_all_positions()
            .map(|(_, pos)| pos)
            .collect();
        let pending_symbols: HashSet<_> = self
            .state
            .orders
            .values()
            .filter(|w| !w.reduce_only)
            .map(|w| &w.order.symbol)
            .filter(|symbol| self.state.positions.get_position(symbol).is_none())
            .collect();
        let pending_heat = self.pending_heat(None);
        let mut sizing_risk = self.state.risk.clone();
        let remaining_heat =
            (sizing_risk.current_capital * sizing_risk.max_portfolio_heat - pending_heat).max(0.0);
        sizing_risk.max_portfolio_heat = if sizing_risk.current_capital > 0.0 {
            remaining_heat / sizing_risk.current_capital
        } else {
            0.0
        };
        let existing = self.state.positions.get_position(&request.symbol);
        let count = positions.len() + pending_symbols.len()
            - usize::from(existing.is_none() && pending_symbols.contains(&request.symbol));
        let (mut order, mut levels, reduce_only) = match size_order(
            &request,
            candles,
            existing,
            count,
            &positions,
            &sizing_risk,
            self.strategy.as_ref(),
        ) {
            SizedOrder::Entry {
                order,
                stop_price,
                target_price,
            } => {
                ensure!(
                    stop_price.is_finite()
                        && target_price.is_finite()
                        && stop_price >= 0.0
                        && target_price > 0.0,
                    "Invalid protective levels"
                );
                (order, Some((stop_price, target_price)), false)
            }
            SizedOrder::Exit(order) => (order, None, true),
            SizedOrder::Rejected(reason) => {
                tracing::debug!(?reason, "Order rejected");
                return Ok(());
            }
        };
        if reduce_only {
            let reserved: Money = self
                .state
                .orders
                .values()
                .filter(|w| w.reduce_only && !w.protective && w.order.symbol == order.symbol)
                .map(|w| w.order.remaining_quantity)
                .sum();
            order.quantity = order
                .quantity
                .min(existing.context("Missing exit position")?.quantity - reserved);
            order.remaining_quantity = order.quantity;
        }
        if !order.quantity.is_positive() {
            return Ok(());
        }
        let price = order
            .limit_price
            .or(order.stop_price)
            .map(|p| p.to_f64())
            .unwrap_or(candle.close);
        if !reduce_only {
            let effective_levels = self.state.levels.get(&order.symbol).copied().or_else(|| {
                self.state
                    .orders
                    .values()
                    .find(|w| !w.reduce_only && w.order.symbol == order.symbol)
                    .and_then(|w| w.levels)
            });
            if let Some((stop, target)) = effective_levels {
                levels = Some((stop, target));
                let allowed = sizing_risk.calculate_position_size_with_regime(
                    price,
                    stop,
                    &positions,
                    self.strategy.get_regime_score(candles),
                );
                order.quantity = order.quantity.min(Money::from_f64(allowed));
            }
            let committed = self.committed_exposure(&order.symbol, candle.close, None);
            let available_value = (self.state.risk.current_capital
                * self.config.trading.max_position_pct
                - committed)
                .max(0.0);
            order.quantity = order.quantity.min(Money::from_f64(available_value / price));
            order.remaining_quantity = order.quantity;
            if !order.quantity.is_positive() {
                tracing::debug!(symbol = %order.symbol, "No remaining position capacity");
                return Ok(());
            }
        }
        ensure!(
            matches!(
                order.order_type,
                OrderType::Market | OrderType::Limit | OrderType::Stop
            ),
            "Unsupported order type: {:?}",
            order.order_type
        );
        ensure!(price.is_finite() && price > 0.0, "Invalid order price");
        if order.side == Side::Buy && !reduce_only {
            let turnover = self.order_execution_price(&order, price) * order.quantity.to_f64();
            let needed = turnover
                + self
                    .execution
                    .estimate_commission(order.side, turnover, order.order_type == OrderType::Limit)
                    .to_f64();
            if needed > self.available_cash() {
                tracing::warn!(symbol = %order.symbol, "Order rejected: insufficient unreserved cash");
                return Ok(());
            }
        }
        let immediate = self.state.mode == ExecutionMode::Simulated
            && order.order_type == OrderType::Market
            && !self.config.backtest.use_t1_execution;
        let price = self.slipped(candle.close, order.side);
        let id = self.place(order, levels, reduce_only, candle.datetime, regime_score);
        if immediate {
            self.simulate_fill(id, price, false, candle.datetime)?;
        }
        Ok(())
    }

    fn place(
        &mut self,
        mut order: Order,
        levels: Option<(f64, f64)>,
        reduce_only: bool,
        timestamp: DateTime<Utc>,
        regime_score: f64,
    ) -> u64 {
        order.created_at = timestamp;
        order.updated_at = timestamp;
        order.state = OrderState::Open;
        let id = order.id;
        self.state.last_order_id = self.state.last_order_id.max(id);
        self.state.orders.insert(
            id,
            WorkingOrder {
                order,
                levels,
                reduce_only,
                regime_score,
                realized: None,
                last_cumulative_fill: None,
                protective: false,
            },
        );
        id
    }

    fn slipped(&self, price: f64, side: Side) -> f64 {
        price
            * (1.0
                + self.config.exchange.assumed_slippage
                    * if side == Side::Buy { 1.0 } else { -1.0 })
    }

    fn order_execution_price(&self, order: &Order, price: f64) -> f64 {
        if order.order_type == OrderType::Limit {
            price
        } else {
            self.slipped(price, order.side)
        }
    }

    fn simulate_fill(
        &mut self,
        id: u64,
        price: f64,
        maker: bool,
        timestamp: DateTime<Utc>,
    ) -> Result<bool> {
        if !self.prepare_fill(id, price, maker)? {
            return Ok(false);
        }
        let mut order = self.state.orders[&id].order.clone();
        let fill = self
            .execution
            .execute_fill(&mut order, price, maker, timestamp);
        self.apply_cumulative_fill(CumulativeFill {
            order_id: id,
            quantity: order.filled_quantity,
            average_price: order.average_fill_price,
            incremental_commission: fill.commission,
            is_maker: maker,
            timestamp,
        })?;
        Ok(true)
    }

    fn prepare_fill(&mut self, id: u64, price: f64, maker: bool) -> Result<bool> {
        let mut order = self.state.orders[&id].order.clone();
        let reduce_only = self.state.orders[&id].reduce_only;
        if reduce_only {
            let quantity = self
                .state
                .positions
                .get_position(&order.symbol)
                .filter(|p| p.side != order.side)
                .map(|p| p.quantity)
                .unwrap_or(Money::ZERO);
            order.remaining_quantity = order.remaining_quantity.min(quantity);
        } else {
            let capital = self.state.risk.current_capital;
            let capacity = self.execution_entry_capacity(id, price, capital)?;
            let fee = self
                .execution
                .estimate_commission(order.side, price * capacity, maker)
                .to_f64();
            let mark = self.state.prices[&order.symbol];
            let adverse_mark = match order.side {
                Side::Buy => (price - mark).max(0.0),
                Side::Sell => (mark - price).max(0.0),
            };
            let net_capital = capital - fee - adverse_mark * capacity;
            let allowed = capacity.min(self.execution_entry_capacity(id, price, net_capital)?);
            if allowed < order.remaining_quantity.to_f64() {
                order.remaining_quantity = order.remaining_quantity.min(Money::from_f64(allowed));
            }
            if !order.remaining_quantity.is_positive() {
                tracing::warn!(
                    id,
                    "Entry cancelled: execution-time protection, risk, or exposure limits"
                );
                self.cancel(id);
                return Ok(false);
            }
            if order.remaining_quantity != self.state.orders[&id].order.remaining_quantity {
                tracing::info!(id, quantity = %order.remaining_quantity, "Entry resized at execution");
                order.quantity = order.filled_quantity + order.remaining_quantity;
                let stored = &mut self
                    .state
                    .orders
                    .get_mut(&id)
                    .context("Missing execution order")?
                    .order;
                stored.quantity = order.quantity;
                stored.remaining_quantity = order.remaining_quantity;
            }
        }
        if !order.remaining_quantity.is_positive() {
            self.cancel(id);
            return Ok(false);
        }
        let turnover = price * order.remaining_quantity.to_f64();
        if order.side == Side::Buy
            && !reduce_only
            && turnover
                + self
                    .execution
                    .estimate_commission(order.side, turnover, maker)
                    .to_f64()
                > self.available_cash_except(Some(id))
        {
            tracing::warn!(id, "Order cancelled: insufficient cash at execution");
            self.cancel(id);
            return Ok(false);
        }
        let stored = &mut self
            .state
            .orders
            .get_mut(&id)
            .context("Missing sized order")?
            .order;
        stored.quantity = stored.filled_quantity + order.remaining_quantity;
        stored.remaining_quantity = order.remaining_quantity;
        Ok(true)
    }

    /// Settle fills through one FIFO/cash/risk path.
    pub fn apply_fill(&mut self, fill: Fill) -> Result<()> {
        let working = self
            .state
            .orders
            .get(&fill.order_id)
            .context("Fill for unknown order")?;
        ensure!(
            fill.quantity.is_positive()
                && fill.price.is_positive()
                && fill.quantity <= working.order.remaining_quantity
                && fill.commission >= Money::ZERO,
            "Invalid fill"
        );
        let symbol = working.order.symbol.clone();
        let side = working.order.side;
        let levels = working.levels;
        if working.reduce_only {
            let position = self
                .state
                .positions
                .get_position(&symbol)
                .context("Exit has no position")?;
            ensure!(
                position.side != side && fill.quantity <= position.quantity,
                "Exit would reverse position"
            );
        } else {
            ensure!(
                self.state
                    .positions
                    .get_position(&symbol)
                    .is_none_or(|position| position.side == side),
                "Entry would reverse an existing position"
            );
        }
        let previous = self.state.positions.get_position(&symbol).map(|p| p.side);
        if self.state.mode == ExecutionMode::ExternalSpot {
            let reduce_only = working.reduce_only;
            if reduce_only {
                let mut left = fill.quantity;
                for lot in &self
                    .state
                    .positions
                    .get_position(&symbol)
                    .context("Missing FIFO position")?
                    .fills
                {
                    let quantity = left.min(lot.quantity);
                    let basis = self
                        .state
                        .fee_basis
                        .get_mut(&lot.order_id)
                        .context("Missing entry fee basis")?;
                    *basis.disposed.entry(fill.order_id).or_insert(Money::ZERO) += quantity;
                    left -= quantity;
                    if left.is_zero() {
                        break;
                    }
                }
                ensure!(left.is_zero(), "FIFO fee allocation mismatch");
            }
            let basis = self
                .state
                .fee_basis
                .entry(fill.order_id)
                .or_insert_with(|| FeeBasis {
                    symbol: symbol.clone(),
                    reduce_only,
                    quantity: Money::ZERO,
                    disposed: BTreeMap::new(),
                    trade_index: None,
                });
            basis.quantity += fill.quantity;
        }
        let trade = self
            .state
            .positions
            .add_fill(fill.clone(), symbol.clone(), side);
        self.state.cash += match side {
            Side::Buy => -(fill.price * fill.quantity + fill.commission).to_f64(),
            Side::Sell => (fill.price * fill.quantity - fill.commission).to_f64(),
        };
        if let Some(levels) = levels {
            self.state.levels.entry(symbol.clone()).or_insert(levels);
        }
        self.state
            .prices
            .entry(symbol.clone())
            .or_insert(fill.price.to_f64());
        let position = self.state.positions.get_position(&symbol);
        let closed = previous.is_some() && position.map(|p| p.side) != previous;
        if let Some(position) = position {
            self.strategy.on_order_filled(&fill, position);
        } else {
            self.state.positions.close_position(&symbol);
            self.state.levels.remove(&symbol);
            self.state.trailing.remove(&symbol);
        }
        let working = self
            .state
            .orders
            .get_mut(&fill.order_id)
            .context("Missing fill order")?;
        let total = working.order.average_fill_price * working.order.filled_quantity
            + fill.price * fill.quantity;
        working.order.filled_quantity += fill.quantity;
        working.order.remaining_quantity -= fill.quantity;
        working.order.average_fill_price = total / working.order.filled_quantity;
        working.order.updated_at = fill.timestamp;
        working.order.state = OrderState::PartiallyFilled;
        working.last_cumulative_fill = Some(CumulativeFill {
            order_id: fill.order_id,
            quantity: working.order.filled_quantity,
            average_price: working.order.average_fill_price,
            incremental_commission: fill.commission,
            is_maker: fill.is_maker,
            timestamp: fill.timestamp,
        });
        if let Some(trade) = trade {
            working.accumulate(trade);
        }
        if closed {
            if let Some(trade) = &working.realized {
                self.strategy.on_trade_closed(trade);
            }
        }
        if working.order.remaining_quantity.is_zero() {
            self.state.cancel_requested.remove(&fill.order_id);
            let mut completed = self
                .state
                .orders
                .remove(&fill.order_id)
                .context("Missing completed order")?;
            self.remember_completed_fill(
                completed
                    .last_cumulative_fill
                    .take()
                    .context("Missing completed execution cursor")?,
            );
            if let Some(trade) = completed.realized.take() {
                if let Some(basis) = self.state.fee_basis.get_mut(&fill.order_id) {
                    basis.trade_index = Some(self.state.trades.len());
                }
                self.record_trade(trade);
            }
        } else if closed {
            self.cancel(fill.order_id);
        }
        self.refresh_risk();
        Ok(())
    }

    /// Convert cumulative exchange price/quantity into an incremental fill.
    pub fn apply_cumulative_fill(&mut self, report: CumulativeFill) -> Result<()> {
        if let Some(completed) = self.state.completed_fills.get(&report.order_id) {
            ensure!(
                *completed == report,
                "Conflicting completed execution report"
            );
            return Ok(());
        }
        let working = self
            .state
            .orders
            .get(&report.order_id)
            .context("Unknown execution order")?;
        let order = &working.order;
        let delta = report.quantity - order.filled_quantity;
        ensure!(
            delta >= Money::ZERO,
            "Exchange filled quantity went backwards"
        );
        if delta == Money::ZERO {
            ensure!(
                working.last_cumulative_fill.as_ref() == Some(&report),
                "Conflicting active execution report"
            );
            return Ok(());
        }
        let notional = report.quantity * report.average_price
            - order.filled_quantity * order.average_fill_price;
        ensure!(
            notional.is_positive(),
            "Invalid incremental exchange notional"
        );
        self.apply_fill(Fill {
            order_id: report.order_id,
            price: notional / delta,
            quantity: delta,
            timestamp: report.timestamp,
            commission: report.incremental_commission,
            is_maker: report.is_maker,
        })?;
        if self.state.completed_fills.contains_key(&report.order_id) {
            self.remember_completed_fill(report);
        } else if let Some(working) = self.state.orders.get_mut(&report.order_id) {
            working.order.average_fill_price = report.average_price;
            working.last_cumulative_fill = Some(report);
        }
        Ok(())
    }

    pub fn execution_intents(&self) -> Vec<ExecutionIntent> {
        self.state
            .orders
            .values()
            .map(|w| ExecutionIntent {
                order: w.order.clone(),
                reduce_only: w.reduce_only,
                protective: w.protective,
                cancel_requested: self.state.cancel_requested.contains(&w.order.id),
                levels: w.levels,
            })
            .collect()
    }

    pub fn protective_level(&self, symbol: &Symbol) -> Result<f64> {
        let (stop, _) = self
            .state
            .levels
            .get(symbol)
            .context("Position has no protection")?;
        Ok(self.state.trailing.get(symbol).copied().unwrap_or(*stop))
    }

    pub fn request_cancel(&mut self, id: u64) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        ensure!(
            self.state.orders.contains_key(&id),
            "Unknown cancellation order"
        );
        self.cancel(id);
        Ok(())
    }

    /// Call only for an unsent intent or an authoritative terminal venue report.
    pub fn confirm_cancel(&mut self, id: u64) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        self.complete_cancel(id);
        self.refresh_risk();
        Ok(())
    }

    pub fn prepare_external(&mut self, id: u64, price: f64) -> Result<bool> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        ensure!(price.is_finite() && price > 0.0, "Invalid execution price");
        let order = &self
            .state
            .orders
            .get(&id)
            .context("Unknown execution order")?
            .order;
        ensure!(
            order.filled_quantity.is_zero(),
            "Cannot resize a submitted/filled order"
        );
        let maker = order.order_type == OrderType::Limit;
        self.prepare_fill(id, price, maker)
    }

    pub fn quantize_external(
        &mut self,
        id: u64,
        request: &super::broker::BrokerOrder,
    ) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        let order = &mut self
            .state
            .orders
            .get_mut(&id)
            .context("Unknown execution order")?
            .order;
        ensure!(
            order.filled_quantity.is_zero()
                && request.quantity.is_positive()
                && request.quantity <= order.quantity
                && request.symbol == order.symbol
                && request.side == order.side
                && request.kind == order.order_type,
            "Invalid external order quantization"
        );
        order.quantity = request.quantity;
        order.remaining_quantity = request.quantity;
        order.limit_price = request.limit;
        order.stop_price = request.stop;
        Ok(())
    }

    pub fn add_native_protection(
        &mut self,
        symbol: &Symbol,
        stop: Money,
        limit: Money,
        timestamp: DateTime<Utc>,
    ) -> Result<u64> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        ensure!(
            !self
                .state
                .orders
                .values()
                .any(|w| w.protective && &w.order.symbol == symbol),
            "A protective order is already working"
        );
        let position = self
            .state
            .positions
            .get_position(symbol)
            .context("No position to protect")?;
        ensure!(
            position.side == Side::Buy && limit.is_positive() && stop > limit,
            "Invalid spot protection"
        );
        let order = Order::new(
            symbol.clone(),
            Side::Sell,
            OrderType::StopLimit,
            position.quantity,
            Some(limit),
            Some(stop),
            super::TimeInForce::GTC,
            None,
        );
        let id = self.place(order, None, true, timestamp, 1.0);
        self.state
            .orders
            .get_mut(&id)
            .context("Missing protective intent")?
            .protective = true;
        Ok(id)
    }

    /// Quotes drive the same risk and protective-exit rules, without fictional fills.
    pub fn on_quotes(
        &mut self,
        quotes: &[super::broker::Quote],
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        for quote in quotes {
            quote.validate(timestamp)?;
            self.state
                .prices
                .insert(quote.symbol.clone(), quote.bid.to_f64());
        }
        self.refresh_risk();
        for quote in quotes {
            let Some(position) = self.state.positions.get_position(&quote.symbol).cloned() else {
                continue;
            };
            let target = self.state.levels[&quote.symbol].1;
            let stop = self.protective_level(&quote.symbol)?;
            let price = quote.bid.to_f64();
            let tick = Candle::new_unchecked(timestamp, price, price, price, price, 0.0);
            if evaluate_exit(position.side, &tick, stop, target).is_some() {
                let ids: Vec<_> = self
                    .state
                    .orders
                    .values()
                    .filter(|w| w.order.symbol == quote.symbol && !w.protective)
                    .map(|w| w.order.id)
                    .collect();
                let market_exit = ids.iter().any(|id| {
                    let w = &self.state.orders[id];
                    w.reduce_only && w.order.order_type == OrderType::Market
                });
                for id in ids {
                    let w = &self.state.orders[&id];
                    if !w.reduce_only || w.order.order_type != OrderType::Market {
                        self.cancel(id);
                    }
                }
                let pending_exit = self
                    .state
                    .orders
                    .values()
                    .any(|w| w.order.symbol == quote.symbol && w.reduce_only && !w.protective);
                if !market_exit && !pending_exit {
                    let order = close_position_order(&quote.symbol, &position).to_order();
                    self.place(order, None, true, timestamp, 1.0);
                }
            }
        }
        if self.state.risk.should_halt_trading() {
            self.cancel_external_entries()?;
        }
        Ok(())
    }

    pub fn cancel_external_entries(&mut self) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot,
            "Not an external engine"
        );
        let ids: Vec<_> = self
            .state
            .orders
            .values()
            .filter(|w| !w.reduce_only)
            .map(|w| w.order.id)
            .collect();
        for id in ids {
            self.cancel(id);
        }
        Ok(())
    }

    /// Late cumulative fee increments follow the original FIFO allocation, including
    /// inventory already sold. No synthetic fill or duplicate strategy callback.
    pub fn apply_external_fee(&mut self, id: u64, fee: Money) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::ExternalSpot && fee.is_positive(),
            "Invalid fee adjustment"
        );
        let basis = self
            .state
            .fee_basis
            .get(&id)
            .context("Unknown fee order")?
            .clone();
        if basis.reduce_only {
            self.adjust_exit_fee(id, fee)?;
        } else {
            let mut allocations: Vec<_> = basis
                .disposed
                .iter()
                .map(|(exit, quantity)| (Some(*exit), *quantity))
                .collect();
            let remaining: Money = self
                .state
                .positions
                .get_position(&basis.symbol)
                .map(|p| {
                    p.fills
                        .iter()
                        .filter(|f| f.order_id == id)
                        .map(|f| f.quantity)
                        .sum()
                })
                .unwrap_or(Money::ZERO);
            if remaining.is_positive() {
                allocations.push((None, remaining));
            }
            ensure!(
                allocations.iter().map(|(_, q)| *q).sum::<Money>() == basis.quantity,
                "Fee allocation quantity mismatch"
            );
            let mut unallocated = fee;
            let last = allocations.len().saturating_sub(1);
            for (index, (exit, quantity)) in allocations.into_iter().enumerate() {
                let share = if index == last {
                    unallocated
                } else {
                    fee * quantity / basis.quantity
                };
                unallocated -= share;
                if let Some(exit) = exit {
                    self.adjust_exit_fee(exit, share)?;
                } else {
                    let position = self
                        .state
                        .positions
                        .get_position_mut(&basis.symbol)
                        .context("Missing fee position")?;
                    let indices: Vec<_> = position
                        .fills
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.order_id == id)
                        .map(|(i, _)| i)
                        .collect();
                    let mut left = share;
                    for (n, index) in indices.iter().enumerate() {
                        let lot = &mut position.fills[*index];
                        let part = if n + 1 == indices.len() {
                            left
                        } else {
                            share * lot.quantity / remaining
                        };
                        lot.commission += part;
                        left -= part;
                    }
                }
            }
        }
        self.state.cash -= fee.to_f64();
        self.state.risk.consecutive_losses = 0;
        self.state.risk.consecutive_wins = 0;
        for trade in &self.state.trades {
            if trade.net_pnl.to_f64() > PNL_EPSILON {
                self.state.risk.record_win();
            } else if trade.net_pnl.to_f64() < -PNL_EPSILON {
                self.state.risk.record_loss();
            }
        }
        self.refresh_risk();
        Ok(())
    }

    fn adjust_exit_fee(&mut self, id: u64, fee: Money) -> Result<()> {
        let basis = self
            .state
            .fee_basis
            .get(&id)
            .context("Missing exit fee basis")?;
        let trade = if let Some(index) = basis.trade_index {
            self.state
                .trades
                .get_mut(index)
                .context("Missing settled fee trade")?
        } else {
            self.state
                .orders
                .get_mut(&id)
                .and_then(|w| w.realized.as_mut())
                .context("Missing partial fee trade")?
        };
        trade.commission += fee;
        trade.net_pnl -= fee;
        Ok(())
    }

    /// Explicit end-of-data liquidation; live adapters do not call this on each tick.
    pub fn finish(&mut self, timestamp: DateTime<Utc>) -> Result<()> {
        ensure!(
            self.state.mode == ExecutionMode::Simulated,
            "Cannot simulate liquidation in external execution mode"
        );
        let ids: Vec<_> = self.state.orders.keys().copied().collect();
        for id in ids {
            self.cancel(id);
        }
        let mut positions: Vec<_> = self
            .state
            .positions
            .get_all_positions()
            .map(|(s, p)| (s.clone(), p.clone()))
            .collect();
        positions.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        for (symbol, pos) in positions {
            let request = close_position_order(&symbol, &pos);
            let price = self.slipped(self.state.prices[&symbol], request.side);
            let id = self.place(request.to_order(), None, true, timestamp, 1.0);
            self.simulate_fill(id, price, false, timestamp)?;
            ensure!(
                self.state.positions.get_position(&symbol).is_none(),
                "Unable to liquidate {symbol}: remaining position {:?}, cash {}, closing price {price}",
                self.state.positions.get_position(&symbol).map(|p| (p.side, p.quantity)),
                self.state.cash
            );
        }
        Ok(())
    }
}
