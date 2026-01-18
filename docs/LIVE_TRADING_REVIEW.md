# Live Trading - Remaining Issues

**Last Updated:** 2026-01-18
**File:** `src/commands/live.rs`

---

## Reviewer Persona

**Name:** Senior Quantitative Systems Engineer
**Experience:** 14 years in ultra-low latency trading systems

### Professional Background

- **Jane Street Capital (2012-2016):** Built market-making infrastructure for ETF arbitrage. Reduced tick-to-trade latency from 800μs to 180μs. Systems handled $2B+ daily notional.

- **Citadel Securities (2016-2020):** Lead engineer on equities execution platform. Deployed strategies with Sharpe >3.0, max drawdown <5%. Managed risk systems for $500M AUM book.

- **Two Sigma (2020-2024):** Principal engineer on crypto systematic strategies. Built exchange connectivity for 12 venues, sub-millisecond fill detection. Strategies generated $40M+ annual PnL with 0.95 Sharpe.

- **Independent (2024-present):** Consulting on trading system architecture. Focus on crash recovery, idempotency, and exchange integration patterns.

### Technical Specializations

| Domain | Metrics/Standards |
|--------|-------------------|
| Latency | P99 < 1ms for order placement, P50 < 100μs for fill detection |
| Reliability | 99.99% uptime, zero duplicate orders, zero missed fills |
| Risk | Max drawdown < 20%, position limits enforced at system level |
| Recovery | Full state restoration < 30s, no manual intervention required |

### Review Philosophy

**"If it can fail silently, it will fail expensively."**

Critical violations that would fail production review:
1. Any code path where order placement succeeds but local state update fails
2. Network timeouts that don't have explicit retry/abort logic
3. Position calculations using f64 without tolerance checks
4. HashMap iteration where order matters for determinism
5. Missing circuit breakers on exchange API calls
6. Shutdown handlers that don't cancel open orders

### Code Standards

```
- Every exchange call: timeout + retry + circuit breaker
- Every state mutation: persist before acknowledge
- Every fill: reconcile against exchange within 1 cycle
- Every error: log with full context, never swallow
- Every number: use Money type or explicit tolerance
```

---

## Remaining Issues

| Priority | Count |
|----------|-------|
| Critical | 1 |
| High | 3 |
| Medium | 3 |
| Minor | 1 |

---

## Critical

### 1. No Idempotency Protection
**Location:** `send_order_to_exchange()` ~line 1050

**Risk:** Network failure after exchange accepts order but before response received. Retry causes double-order. Could double position size, double losses.

**Required Fix:**
1. Generate idempotency key before send: `let idem_key = format!("{}_{}", timestamp_ms, order_id)`
2. Persist to SQLite with status `pending_send` BEFORE calling exchange
3. On exchange response: update status to `sent` with exchange_id
4. On startup: query exchange for orders matching our `strat_*` prefix, reconcile

```rust
// BEFORE exchange call
self.state_manager.save_order_attempt(&idem_key, &order)?;

// Exchange call
let result = self.exchange.place_order(&order_req).await;

// AFTER - update based on result
match result {
    Ok(resp) => self.state_manager.mark_order_sent(&idem_key, &resp.id)?,
    Err(e) => self.state_manager.mark_order_failed(&idem_key, &e.to_string())?,
}
```

---

## High Priority

### 2. Sequential Order Polling (N+1 Problem)
**Location:** `poll_pending_orders()` ~line 1217

**Risk:** 10 pending orders = 10 sequential API calls = 10+ seconds per cycle. Rate limit exhaustion, stale fill detection.

**Required Fix:**
```rust
use futures::future::join_all;

let order_ids: Vec<_> = self.pending_exchange_orders.keys().cloned().collect();
let futures = order_ids.iter().map(|id| self.exchange.get_order_status(id));
let results: Vec<Result<_, _>> = join_all(futures).await;

for (id, result) in order_ids.iter().zip(results) {
    // Process each result
}
```

---

### 3. No Duplicate Order Protection
**Location:** Order placement loop ~line 950

**Risk:** Strategy generates identical signal twice due to lag. Two orders placed. Position 2x intended size.

**Required Fix:**
```rust
// Add to LiveTrader
recent_orders: HashSet<(Symbol, Side, i64)>, // (symbol, side, qty_micros)

// Before placing
let key = (order.symbol.clone(), order.side, (qty * 1e6) as i64);
if !self.recent_orders.insert(key) {
    warn!("Duplicate order blocked: {:?}", key);
    continue;
}

// Clear at cycle end
self.recent_orders.clear();
```

---

### 4. Stale Data Not Detected
**Location:** `update_candles()` ~line 640

**Risk:** Candle fetch fails silently. Trading continues on hour-old data. Stop losses calculated on stale prices.

**Required Fix:**
```rust
// After candle update
if let Some(last_candle) = candles.last() {
    let age = Utc::now() - last_candle.datetime;
    let max_stale = match timeframe {
        "1m" => Duration::minutes(5),
        "1h" => Duration::hours(2),
        "1d" => Duration::days(2),
        _ => Duration::hours(1),
    };
    if age > max_stale {
        error!("Data stale for {}: age={}, skipping", symbol, age);
        return Ok(()); // Skip this symbol
    }
}
```

---

## Medium Priority

### 5. No Pre-Flight Health Check
**Location:** `run()` before main loop ~line 490

**Risk:** Trading starts with invalid credentials, unreachable exchange, or clock drift. Orders fail, positions desync.

**Required Fix:**
```rust
async fn preflight_check(&self) -> Result<()> {
    // 1. Exchange reachable
    let server_time = self.exchange.get_server_time().await?;

    // 2. Clock sync < 5s drift
    let drift_ms = (Utc::now().timestamp_millis() - server_time).abs();
    ensure!(drift_ms < 5000, "Clock drift {}ms exceeds 5s limit", drift_ms);

    // 3. Credentials valid
    self.exchange.get_balances().await?;

    // 4. Sufficient balance
    let inr = self.exchange_balances.get("INR").unwrap_or(&0.0);
    ensure!(*inr >= 100.0, "INR balance {:.2} below minimum", inr);

    Ok(())
}
```

---

### 6. No Kill Switch
**Location:** Missing feature

**Risk:** Runaway strategy, exchange issues, or bug requires immediate exit. No way to close all positions and cancel orders atomically.

**Required Fix:**
```rust
async fn emergency_exit(&mut self) -> Result<()> {
    error!("EMERGENCY EXIT");

    // 1. Cancel all pending
    for id in self.pending_exchange_orders.keys().cloned().collect::<Vec<_>>() {
        let _ = self.exchange.cancel_order(&id).await;
    }

    // 2. Flatten all positions
    for (sym, pos) in self.position_manager.get_all_positions() {
        if !pos.quantity.is_zero() {
            let order = match pos.side {
                Side::Buy => OrderRequest::market_sell(sym.clone(), pos.quantity.to_f64()),
                Side::Sell => OrderRequest::market_buy(sym.clone(), pos.quantity.to_f64()),
            };
            let _ = self.send_order_to_exchange(&order.to_order()).await;
        }
    }

    // 3. Force halt
    self.risk_manager.force_halt();
    Ok(())
}
```

Wire to: double Ctrl+C, SIGUSR1, or config flag file.

---

### 7. Non-Deterministic Iteration
**Location:** HashMap declarations ~line 152

**Risk:** Debug logs show symbols in random order. Harder to diff logs across runs. Non-reproducible behavior in edge cases.

**Required Fix:**
```rust
// In types.rs - add Ord
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, ...)]
pub struct Symbol(...)

// In live.rs - use BTreeMap
use std::collections::BTreeMap;
orderbooks: BTreeMap<Symbol, OrderBook>,
entry_levels: BTreeMap<Symbol, (f64, f64)>,
trailing_stops: BTreeMap<Symbol, f64>,
```

---

## Minor

### 8. Position Reconciliation Over-Aggressive
**Location:** `reconcile_positions()` ~line 1385

**Risk:** Exchange balance API returns 0 (glitch). We clear local position. Actual position still exists. State desync.

**Required Fix:**
```rust
} else if local_qty > 0.0 && exchange_qty == 0.0 {
    // Don't trust zero - check for active orders first
    let active = self.exchange.get_active_orders(sym).await?;
    if active.is_empty() {
        warn!("Clearing orphan local position for {}", symbol);
        self.position_manager.close_position(symbol);
    } else {
        warn!("Local position exists, active orders found - NOT clearing {}", symbol);
    }
}
```

---

## Review Checklist for Future Implementation

- [ ] 1: Idempotency - SQLite before send, reconcile on startup
- [ ] 2: Parallel polling - join_all or batch API
- [ ] 3: Duplicate protection - HashSet of recent order keys
- [ ] 4: Staleness check - skip symbol if data too old
- [ ] 5: Preflight - connectivity, clock, credentials, balance
- [ ] 6: Kill switch - cancel all, flatten all, halt
- [ ] 7: BTreeMap - add Ord to Symbol, replace HashMap
- [ ] 8: Safe reconciliation - verify no active orders before clearing
