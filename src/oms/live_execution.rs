//! Durable, single-writer spot execution. The journal and engine commit together.

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;

use super::broker::{
    balance, Balance, Broker, BrokerOrder, BrokerReport, BrokerStatus, MarketRules, Quote,
};
use super::trading_engine::{CumulativeFill, ExecutionIntent};
use super::{EngineSnapshot, OrderType, TradingEngine};
use crate::state_manager::SqliteStateManager;
use crate::{Config, Money, MultiSymbolMultiTimeframeData, Side, Strategy, Symbol};

const VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
#[error("Live state persistence failed; no further mutations are safe: {0:#}")]
struct DurabilityError(#[source] anyhow::Error);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Submitting,
    Working,
    Cancelling,
    Terminal,
}

#[derive(Clone, Serialize, Deserialize)]
struct JournalOrder {
    request: BrokerOrder,
    protective: bool,
    phase: Phase,
    report: Option<BrokerReport>,
    attempted_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    namespace: String,
    orders: BTreeMap<u64, JournalOrder>,
    quote_reserve: Money,
    halt: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    identity: serde_json::Value,
    engine: EngineSnapshot,
    journal: Journal,
}

pub struct LiveExecution<B> {
    engine: TradingEngine,
    broker: B,
    db: SqliteStateManager,
    identity: serde_json::Value,
    rules: BTreeMap<String, MarketRules>,
    journal: Journal,
    _account_lease: File,
}

impl<B: Broker> LiveExecution<B> {
    pub async fn open(
        config: Config,
        strategy: Box<dyn Strategy>,
        broker: B,
        db: SqliteStateManager,
    ) -> Result<Self> {
        let symbols: Vec<_> = config.trading.symbols.iter().map(Symbol::new).collect();
        let markets = broker
            .markets(&symbols)
            .await
            .context("Fetching execution market rules")?;
        let mut rules = BTreeMap::new();
        for symbol in &config.trading.symbols {
            let mut matches = markets.iter().filter(|m| m.symbol.as_str() == symbol);
            let market = matches
                .next()
                .with_context(|| format!("No active market {symbol}"))?;
            ensure!(matches.next().is_none(), "Ambiguous market {symbol}");
            market.validate()?;
            ensure!(
                rules.insert(symbol.clone(), market.clone()).is_none(),
                "Duplicate configured symbol"
            );
        }
        let quote = rules
            .values()
            .next()
            .context("No live markets")?
            .quote
            .clone();
        ensure!(
            rules.values().all(|m| m.quote == quote),
            "Live markets must share one quote currency"
        );
        let assets: BTreeSet<_> = rules.values().map(|m| &m.asset).collect();
        ensure!(
            assets.len() == rules.len(),
            "Multiple markets share the same inventory"
        );
        let account = hex::encode(Sha256::digest(broker.account_key().await?.as_bytes()));
        let lease = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(std::env::temp_dir().join(format!("crypto-strategies-account-{account}.lock")))
            .context("Opening account execution lock")?;
        lease
            .try_lock()
            .context("Another local process is using this broker account")?;
        let mut public_config = config.clone();
        public_config.exchange.api_key = None;
        public_config.exchange.api_secret = None;
        let identity = serde_json::json!({"account": account, "config": public_config});
        let mut engine = TradingEngine::new_external_spot(config, strategy)?;
        let journal = if let Some(saved) = db.load_engine_snapshot::<Snapshot>()? {
            ensure!(
                saved.version == VERSION && saved.identity == identity,
                "Live snapshot/account/config mismatch"
            );
            engine.restore(saved.engine)?;
            saved.journal
        } else {
            ensure!(
                !db.has_legacy_state()?,
                "Legacy state requires manual reconciliation; use a new state database"
            );
            let balances = broker.balances().await?;
            validate_balances(&balances)?;
            let cash = balance(&balances, &quote)?;
            ensure!(
                cash.available >= Money::from_f64(engine.cash()) && cash.locked.is_zero(),
                "Insufficient free quote balance or unmanaged locked funds"
            );
            for rule in rules.values() {
                ensure!(asset_balance(&balances, &rule.asset).is_zero(), "Unmanaged {} inventory; a fresh live state requires no holdings in configured assets", rule.asset);
                ensure!(
                    broker.active_orders(&rule.symbol).await?.is_empty(),
                    "Unmanaged orders for {}",
                    rule.symbol
                );
            }
            Journal {
                namespace: db.execution_namespace()?,
                orders: BTreeMap::new(),
                quote_reserve: cash.total() - Money::from_f64(engine.cash()),
                halt: None,
            }
        };
        ensure!(
            journal.namespace.len() == 32
                && journal.namespace.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid execution namespace"
        );
        let this = Self {
            engine,
            broker,
            db,
            identity,
            rules,
            journal,
            _account_lease: lease,
        };
        this.validate_journal()?;
        this.persist().await?;
        Ok(this)
    }

    pub fn engine(&self) -> &TradingEngine {
        &self.engine
    }
    pub fn rules(&self) -> &BTreeMap<String, MarketRules> {
        &self.rules
    }
    pub fn halted(&self) -> Option<&str> {
        self.journal.halt.as_deref()
    }

    fn validate_journal(&self) -> Result<()> {
        let intents = self.engine.execution_intents();
        for (id, record) in &self.journal.orders {
            ensure!(
                record.request.client_id == format!("cs-{}-{id}", self.journal.namespace),
                "Invalid client ID mapping"
            );
            ensure!(
                self.rules.contains_key(record.request.symbol.as_str()),
                "Unknown journal market"
            );
            if let Some(report) = &record.report {
                report.validate(&record.request)?;
            }
            if record.phase == Phase::Terminal {
                ensure!(
                    record.report.as_ref().is_some_and(|r| r.status.terminal()),
                    "Missing terminal report"
                );
                ensure!(
                    !intents.iter().any(|i| i.order.id == *id),
                    "Terminal journal order is still active"
                );
            } else {
                let intent = intents
                    .iter()
                    .find(|i| i.order.id == *id)
                    .context("Journal references a missing engine order")?;
                ensure!(
                    intent.order.quantity == record.request.quantity
                        && intent.order.symbol == record.request.symbol
                        && intent.order.side == record.request.side
                        && intent.order.limit_price == record.request.limit
                        && intent.order.stop_price == record.request.stop
                        && intent.order.order_type == record.request.kind
                        && intent.protective == record.protective,
                    "Engine/journal payload mismatch"
                );
                let filled = record.report.as_ref().map_or(Money::ZERO, |r| r.filled);
                ensure!(
                    intent.order.filled_quantity == filled,
                    "Engine/journal execution cursor mismatch"
                );
            }
        }
        Ok(())
    }

    async fn persist(&self) -> Result<()> {
        let result = async {
            let snapshot = Snapshot {
                version: VERSION,
                identity: self.identity.clone(),
                engine: self.engine.snapshot()?,
                journal: self.journal.clone(),
            };
            let db = self.db.clone();
            tokio::task::spawn_blocking(move || db.save_engine_snapshot(&snapshot))
                .await
                .context("Live persistence worker failed")?
        }
        .await;
        result.map_err(|error| DurabilityError(error).into())
    }

    async fn protection_error(&mut self, error: anyhow::Error) -> Result<()> {
        if error.is::<DurabilityError>() {
            return Err(error);
        }
        self.halt(format!("Protection/exit failed: {error:#}"))
            .await
    }

    pub async fn halt(&mut self, reason: impl Into<String>) -> Result<()> {
        let reason = reason.into();
        tracing::error!(%reason, "Live entries halted; existing inventory still requires reconciliation/protection");
        if self.journal.halt.is_none() {
            self.journal.halt = Some(reason);
        }
        self.engine.cancel_external_entries()?;
        self.persist().await
    }

    /// Read-only with respect to the broker: lookup, balance and ownership checks only.
    pub async fn preflight(&mut self) -> Result<()> {
        let unresolved = self.reconcile(false).await?;
        ensure!(
            unresolved.is_empty(),
            "Orders have unresolved venue outcomes"
        );
        self.reconcile_account().await?;
        self.persist().await
    }

    pub async fn resume(&mut self) -> Result<()> {
        self.preflight().await?;
        ensure!(
            self.protected(),
            "Cannot resume entries before every position has acknowledged native protection"
        );
        self.journal.halt = None;
        self.persist().await
    }

    pub async fn on_bar(
        &mut self,
        data: &MultiSymbolMultiTimeframeData,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        ensure!(self.journal.halt.is_none(), "Live execution is halted");
        let before = self.engine.snapshot()?;
        if let Err(error) = self.engine.on_bar(data, timestamp) {
            self.engine.restore(before)?;
            self.halt(format!("Strategy decision failed: {error:#}"))
                .await?;
            return Err(error);
        }
        self.persist().await
    }

    async fn reconcile(&mut self, audit_terminal: bool) -> Result<BTreeSet<String>> {
        let now = Utc::now();
        let ids: Vec<_> = self
            .journal
            .orders
            .iter()
            .filter(|(_, r)| {
                audit_terminal
                    || r.phase != Phase::Terminal
                    || r.report
                        .as_ref()
                        .is_some_and(|report| now - report.updated_at < Duration::minutes(2))
            })
            .map(|(id, _)| *id)
            .collect();
        let mut unresolved = BTreeSet::new();
        for id in ids {
            let record = &self.journal.orders[&id];
            let client_id = record.request.client_id.clone();
            let symbol = record.request.symbol.as_str().to_owned();
            match self.broker.lookup(&client_id).await {
                Ok(Some(report)) => {
                    if let Err(error) = self.accept_report(id, report).await {
                        unresolved.insert(symbol);
                        self.halt(format!("Invalid execution report: {error:#}"))
                            .await?;
                    }
                }
                Ok(None) => {
                    unresolved.insert(symbol);
                    self.halt(format!("Order {client_id} is not found after a send intent; it will not be resubmitted")).await?;
                }
                Err(error) => {
                    unresolved.insert(symbol);
                    self.halt(format!("Order lookup failed: {error:#}")).await?;
                }
            }
        }
        Ok(unresolved)
    }

    async fn accept_report(&mut self, id: u64, report: BrokerReport) -> Result<()> {
        let record = self
            .journal
            .orders
            .get(&id)
            .context("Unjournaled broker report")?;
        report.validate(&record.request)?;
        if record.report.as_ref() == Some(&report) {
            return Ok(());
        }
        let (quantity, price, fee) = record
            .report
            .as_ref()
            .map_or((Money::ZERO, Money::ZERO, Money::ZERO), |r| {
                (r.filled, r.average_price, r.fee)
            });
        if let Some(previous) = &record.report {
            ensure!(
                report.exchange_id == previous.exchange_id,
                "Exchange ID changed"
            );
            ensure!(
                !previous.status.terminal() || report.status == previous.status,
                "Terminal order changed state"
            );
        }
        ensure!(
            report.filled >= quantity && report.fee >= fee,
            "Cumulative fill/fee regressed"
        );
        if report.filled == quantity {
            ensure!(
                report.average_price == price,
                "Same-quantity price correction needs manual reconciliation"
            );
        }
        let before = self.engine.snapshot()?;
        let settlement = (|| -> Result<()> {
            if report.filled > quantity {
                self.engine.apply_cumulative_fill(CumulativeFill {
                    order_id: id,
                    quantity: report.filled,
                    average_price: report.average_price,
                    incremental_commission: report.fee - fee,
                    is_maker: false,
                    timestamp: report.updated_at,
                })?;
            } else if report.fee > fee {
                self.engine.apply_external_fee(id, report.fee - fee)?;
            }
            if report.status.terminal() {
                self.engine.confirm_cancel(id)?;
            }
            Ok(())
        })();
        if let Err(error) = settlement {
            self.engine.restore(before)?;
            return Err(error);
        }
        let record = self
            .journal
            .orders
            .get_mut(&id)
            .context("Missing settled journal order")?;
        record.phase = if report.status.terminal() {
            Phase::Terminal
        } else if record.phase == Phase::Cancelling {
            Phase::Cancelling
        } else {
            Phase::Working
        };
        let rejected = report.status == BrokerStatus::Rejected;
        record.report = Some(report);
        self.persist().await?;
        if rejected {
            self.halt(format!("Venue rejected order {id}")).await?;
        }
        Ok(())
    }

    async fn verify_account(&self) -> Result<()> {
        for rule in self.rules.values() {
            for report in self.broker.active_orders(&rule.symbol).await? {
                let record = self
                    .journal
                    .orders
                    .values()
                    .find(|r| r.request.client_id == report.order.client_id)
                    .context("Unmanaged active order on a configured market")?;
                report.validate(&record.request)?;
                ensure!(
                    record.phase != Phase::Terminal,
                    "Terminal order reappeared as active"
                );
            }
        }
        let balances = self.broker.balances().await?;
        validate_balances(&balances)?;
        let quote = &self.rules.values().next().context("No markets")?.quote;
        let actual = balance(&balances, quote)?.total();
        let expected = Money::from_f64(self.engine.cash()) + self.journal.quote_reserve;
        // The shared core's cash is f64; tolerate only its conversion error, not
        // fees, taxes, deposits, withdrawals or unexplained wallet adjustments.
        let tolerance =
            Money::from_f64((expected.to_f64().abs() * f64::EPSILON * 32.0).max(0.00000001));
        ensure!((actual - expected).abs() <= tolerance, "Quote balance mismatch: wallet {actual}, ledger {expected}; reconcile fees/taxes/external activity");
        for rule in self.rules.values() {
            let expected = self
                .engine
                .positions()
                .get_position(&rule.symbol)
                .map_or(Money::ZERO, |p| p.quantity);
            ensure!(
                asset_balance(&balances, &rule.asset) == expected,
                "Inventory mismatch for {}",
                rule.asset
            );
        }
        Ok(())
    }

    async fn reconcile_account(&mut self) -> Result<bool> {
        if let Err(error) = self.verify_account().await {
            tracing::warn!(
                "Account differs from ledger; auditing retained terminal reports: {error:#}"
            );
            ensure!(
                self.reconcile(true).await?.is_empty(),
                "Terminal audit has unresolved orders"
            );
            self.verify_account().await?;
            return Ok(true);
        }
        Ok(false)
    }

    fn protected(&self) -> bool {
        self.engine
            .positions()
            .get_all_positions()
            .all(|(symbol, position)| {
                self.journal.orders.values().any(|r| {
                    r.protective
                        && r.phase == Phase::Working
                        && &r.request.symbol == symbol
                        && r.report.as_ref().is_some_and(|report| {
                            !report.status.terminal()
                                && r.request.quantity - report.filled == position.quantity
                        })
                })
            })
    }

    fn uncertain(&self) -> bool {
        self.journal
            .orders
            .values()
            .any(|r| matches!(r.phase, Phase::Submitting | Phase::Cancelling))
    }

    async fn cancel(&mut self, id: u64) -> Result<()> {
        if !self.journal.orders.contains_key(&id) {
            self.engine.confirm_cancel(id)?;
            return self.persist().await;
        }
        let record = self
            .journal
            .orders
            .get_mut(&id)
            .context("Missing cancellation journal")?;
        if record.phase == Phase::Submitting {
            return Ok(());
        }
        if record.phase == Phase::Terminal {
            return Ok(());
        }
        if record.phase == Phase::Cancelling
            && Utc::now() - record.attempted_at < Duration::seconds(10)
        {
            return Ok(());
        }
        // A cancel retry is allowed only after lookup confirms the order remains
        // active; it cannot create another order or release engine reservations.
        record.phase = Phase::Cancelling;
        record.attempted_at = Utc::now();
        let client_id = record.request.client_id.clone();
        self.persist().await?;
        if let Err(error) = self.broker.cancel(&client_id).await {
            self.halt(format!("Cancellation outcome unknown: {error:#}"))
                .await?;
        }
        Ok(())
    }

    async fn send(&mut self, intent: ExecutionIntent, price: Money) -> Result<()> {
        let id = intent.order.id;
        ensure!(
            !self.journal.orders.contains_key(&id),
            "Attempted to resubmit a journaled order"
        );
        if !self.engine.prepare_external(id, price.to_f64())? {
            self.engine.confirm_cancel(id)?;
            return self.persist().await;
        }
        let current = self
            .engine
            .execution_intents()
            .into_iter()
            .find(|i| i.order.id == id)
            .context("Missing prepared order")?;
        let rule = &self.rules[current.order.symbol.as_str()];
        let request = match rule.order(
            &current.order,
            format!("cs-{}-{id}", self.journal.namespace),
            price,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.engine.confirm_cancel(id)?;
                self.halt(format!("Order fails venue rules: {error:#}"))
                    .await?;
                return Ok(());
            }
        };
        if intent.protective {
            ensure!(
                request.quantity == current.order.quantity,
                "Cannot protect a fractional dust remainder"
            );
        } else if !intent.reduce_only {
            let inventory = self
                .engine
                .positions()
                .get_position(&request.symbol)
                .map_or(Money::ZERO, |p| p.quantity);
            let pending: Money = self
                .engine
                .execution_intents()
                .iter()
                .filter(|i| !i.reduce_only && i.order.symbol == request.symbol && i.order.id != id)
                .map(|i| i.order.remaining_quantity)
                .sum();
            ensure!(
                inventory + pending + request.quantity <= rule.max_market_quantity.min(rule.max_quantity),
                "Aggregate inventory would exceed a single native protection/market-exit quantity limit"
            );
        }
        let balances = self.broker.balances().await?;
        validate_balances(&balances)?;
        if request.side == Side::Buy {
            ensure!(
                balance(&balances, &rule.quote)?.available
                    >= request.quantity * request.limit.unwrap_or(price),
                "Insufficient available quote funds"
            );
        } else {
            ensure!(
                balance(&balances, &rule.asset)?.available >= request.quantity,
                "Inventory is still locked; cancellation is not settled"
            );
        }
        self.engine.quantize_external(id, &request)?;
        self.journal.orders.insert(
            id,
            JournalOrder {
                request: request.clone(),
                protective: intent.protective,
                phase: Phase::Submitting,
                report: None,
                attempted_at: Utc::now(),
            },
        );
        self.persist().await?;
        match self.broker.submit(&request).await {
            Ok(report) => self.accept_report(id, report).await?,
            Err(error) => {
                self.halt(format!(
                    "Submission outcome unknown for {}: {error:#}; lookup only, no retry",
                    request.client_id
                ))
                .await?
            }
        }
        Ok(())
    }

    pub async fn cycle(&mut self) -> Result<()> {
        let unresolved = self.reconcile(false).await?;
        for intent in self.engine.execution_intents() {
            if self
                .journal
                .orders
                .get(&intent.order.id)
                .is_some_and(|r| r.phase == Phase::Working)
                && (intent.order.filled_quantity.is_positive()
                    || (intent.reduce_only && intent.order.order_type == OrderType::Limit))
                && !intent.protective
            {
                // Freeze partial entries and reductions before reprotecting inventory.
                self.engine.request_cancel(intent.order.id)?;
                self.persist().await?;
            }
        }
        for intent in self.engine.execution_intents() {
            if intent.cancel_requested && !unresolved.contains(intent.order.symbol.as_str()) {
                self.cancel(intent.order.id).await?;
            }
        }
        let symbols: Vec<_> = self.rules.values().map(|m| m.symbol.clone()).collect();
        self.repair_protections(&unresolved).await?;
        let quotes = self.broker.quotes(&symbols).await?;
        ensure!(quotes.len() == symbols.len(), "Incomplete quote frame");
        for symbol in &symbols {
            ensure!(
                quotes.iter().filter(|q| &q.symbol == symbol).count() == 1,
                "Missing/duplicate quote"
            );
        }
        self.engine.on_quotes(&quotes, Utc::now())?;
        if self.engine.risk().should_halt_trading() && self.journal.halt.is_none() {
            self.halt("Portfolio risk halt").await?;
        }
        self.persist().await?;
        for intent in self.engine.execution_intents() {
            if intent.cancel_requested && !unresolved.contains(intent.order.symbol.as_str()) {
                self.cancel(intent.order.id).await?;
            }
        }
        // Protection and exits take precedence over all new entries.
        for quote in &quotes {
            if unresolved.contains(quote.symbol.as_str()) {
                continue;
            }
            if let Err(error) = self.protect_or_exit(quote).await {
                self.protection_error(error).await?;
            }
        }
        if !unresolved.is_empty()
            || self.uncertain()
            || !self.protected()
            || self.journal.halt.is_some()
        {
            return Ok(());
        }
        match self.reconcile_account().await {
            Ok(true) => {
                // An audit can discover new fills as well as late fees; the
                // pre-audit protection/entry gates are no longer authoritative.
                self.repair_protections(&BTreeSet::new()).await?;
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => {
                self.halt(format!("Account reconciliation failed: {error:#}"))
                    .await?;
                return Ok(());
            }
        }
        for intent in self.engine.execution_intents() {
            if intent.reduce_only
                || intent.cancel_requested
                || self.journal.orders.contains_key(&intent.order.id)
            {
                continue;
            }
            let quote = quotes
                .iter()
                .find(|q| q.symbol == intent.order.symbol)
                .context("Missing entry quote")?;
            if let Some((stop, _)) = intent.levels {
                let rule = &self.rules[intent.order.symbol.as_str()];
                let (trigger, limit) = rule.protective_prices(stop)?;
                let mut protection = rule.order(&intent.order, "preflight".into(), quote.ask)?;
                protection.side = Side::Sell;
                protection.kind = OrderType::StopLimit;
                protection.stop = Some(trigger);
                protection.limit = Some(limit);
                rule.validate_order(&protection, quote.bid)?;
            }
            self.send(intent, quote.ask).await?;
            self.protect_or_exit(quote).await?;
            // Reconcile/protect any immediate fill before another entry is sent.
            break;
        }
        Ok(())
    }

    async fn protect_or_exit(&mut self, quote: &Quote) -> Result<()> {
        let intents: Vec<_> = self
            .engine
            .execution_intents()
            .into_iter()
            .filter(|i| i.order.symbol == quote.symbol)
            .collect();
        let position = self.engine.positions().get_position(&quote.symbol).cloned();
        let Some(position) = position else {
            for intent in intents.iter().filter(|i| i.reduce_only) {
                self.cancel(intent.order.id).await?;
            }
            return Ok(());
        };
        let executable = |intent: &&ExecutionIntent| {
            intent.reduce_only
                && !intent.protective
                && !intent.cancel_requested
                && match intent.order.order_type {
                    OrderType::Market => true,
                    OrderType::Limit => intent.order.limit_price.is_some_and(|p| quote.bid >= p),
                    OrderType::Stop => intent.order.stop_price.is_some_and(|p| quote.bid <= p),
                    OrderType::StopLimit => false,
                }
        };
        let mut exit = intents.iter().find(executable).cloned();
        if exit.is_some() {
            let entries: Vec<_> = intents.iter().filter(|i| !i.reduce_only).collect();
            if !entries.is_empty() {
                for entry in entries {
                    self.engine.request_cancel(entry.order.id)?;
                    self.persist().await?;
                    self.cancel(entry.order.id).await?;
                }
                exit = None;
            }
        }
        let working_exit = intents.iter().any(|i| {
            !i.protective && i.reduce_only && self.journal.orders.contains_key(&i.order.id)
        });
        let protection = intents.iter().find(|i| i.protective);
        let rule = &self.rules[quote.symbol.as_str()];
        if let Some(exit) = &exit {
            if !self.journal.orders.contains_key(&exit.order.id) {
                // Never dismantle a valid stop for an exit the venue will reject.
                let mut order = exit.order.clone();
                order.quantity = order.quantity.min(position.quantity);
                if let Err(error) = rule.order(&order, "exit-preflight".into(), quote.bid) {
                    self.halt(format!(
                        "Exit fails venue rules; retaining native protection: {error:#}"
                    ))
                    .await?;
                    return Ok(());
                }
            }
        }
        if let Some(protection) = protection {
            if exit.is_some() || working_exit {
                self.engine.request_cancel(protection.order.id)?;
                self.persist().await?;
                return self.cancel(protection.order.id).await;
            }
            return self.refresh_protection(&quote.symbol).await;
        }
        if working_exit {
            return Ok(());
        }
        if let Some(exit) = exit {
            self.send(exit, quote.bid).await?;
        } else {
            self.refresh_protection(&quote.symbol).await?;
        }
        Ok(())
    }

    async fn refresh_protection(&mut self, symbol: &Symbol) -> Result<()> {
        let position = self
            .engine
            .positions()
            .get_position(symbol)
            .context("Missing protection inventory")?;
        let quantity = position.quantity;
        let (stop, limit) = self.rules[symbol.as_str()]
            .validate_protection(quantity, self.engine.protective_level(symbol)?)?;
        let existing = self
            .engine
            .execution_intents()
            .into_iter()
            .find(|i| &i.order.symbol == symbol && i.protective);
        if let Some(protection) = existing {
            if protection.cancel_requested {
                return Ok(());
            }
            if protection.order.remaining_quantity != quantity
                || protection.order.stop_price != Some(stop)
            {
                self.engine.request_cancel(protection.order.id)?;
                self.persist().await?;
                return self.cancel(protection.order.id).await;
            }
            if !self.journal.orders.contains_key(&protection.order.id) {
                self.send(protection, stop).await?;
            }
            return Ok(());
        }
        let id = self
            .engine
            .add_native_protection(symbol, stop, limit, Utc::now())?;
        self.persist().await?;
        let intent = self
            .engine
            .execution_intents()
            .into_iter()
            .find(|i| i.order.id == id)
            .context("Missing protection intent")?;
        self.send(intent, stop).await
    }

    async fn repair_protections(&mut self, unresolved: &BTreeSet<String>) -> Result<()> {
        // A ticker outage must not prevent protecting a just-reconciled fill.
        // Stop payloads use the approved level, not a fabricated fresh quote.
        let symbols: Vec<_> = self.rules.values().map(|m| m.symbol.clone()).collect();
        for symbol in &symbols {
            if unresolved.contains(symbol.as_str())
                || self.engine.positions().get_position(symbol).is_none()
            {
                continue;
            }
            let intents = self.engine.execution_intents();
            let has_protection = intents
                .iter()
                .any(|i| &i.order.symbol == symbol && i.protective);
            if !has_protection
                && intents
                    .iter()
                    .any(|i| &i.order.symbol == symbol && i.reduce_only)
            {
                continue;
            }
            if let Err(error) = self.refresh_protection(symbol).await {
                self.protection_error(error).await?;
            }
        }
        Ok(())
    }

    pub async fn prepare_shutdown(&mut self) -> Result<()> {
        self.halt("Operator shutdown").await?;
        for intent in self.engine.execution_intents() {
            if !intent.protective {
                self.engine.request_cancel(intent.order.id)?;
            }
        }
        self.persist().await
    }

    pub fn shutdown_ready(&self) -> bool {
        self.protected()
            && !self.uncertain()
            && self.engine.execution_intents().iter().all(|i| i.protective)
    }
}

fn validate_balances(balances: &[Balance]) -> Result<()> {
    let mut currencies = BTreeSet::new();
    for value in balances {
        ensure!(
            !value.currency.is_empty()
                && currencies.insert(&value.currency)
                && value.available >= Money::ZERO
                && value.locked >= Money::ZERO,
            "Invalid/duplicate account balance"
        );
    }
    Ok(())
}

fn asset_balance(balances: &[Balance], currency: &str) -> Money {
    // Spot wallets may omit currencies with zero holdings.
    balances
        .iter()
        .find(|b| b.currency == currency)
        .map_or(Money::ZERO, Balance::total)
}
