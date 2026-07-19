# CoinDCX Live Trading Readiness

**Last updated:** 2026-07-19
**Scope:** `src/commands/live.rs`, OMS, risk management, persistence, and CoinDCX integration

## Current Decision

**Not approved for unattended real-money deployment.**

The local execution, accounting, lifecycle, and exchange-order reconciliation paths
have been corrected and independently reviewed. However, operational protections
listed below remain mandatory before live capital is enabled. Paper mode is the
supported deployment stage.

The current paper candidate is
`configs/sol_bnb_regime_grid_candidate.json`. Its holdout result is positive under
the configured 0.1% fee/0.1% slippage assumptions, but it fails the conservative
0.5% retail-fee stress. Fee-tier verification is therefore a hard deployment gate.

## Corrected and Verified

- Paper fills settle cash, commissions, positions, and trade-close callbacks.
- Live orders remain visible in the local orderbook but are filled only from
  CoinDCX status responses; local candle simulation is disabled in live mode.
- Internal order IDs are mapped to CoinDCX exchange IDs for completion and
  cancellation reconciliation.
- Failed CoinDCX placement removes the local order; failed cancellation restores it.
- Pending market exits are latched to prevent repeated stop/target submissions.
- Multiple grid limit orders remain supported and are not mistaken for duplicate exits.
- `Strategy::init`, `on_bar`, `on_order_filled`, and `on_trade_closed` are driven
  consistently.
- Long and short trailing stops tighten in the correct direction.
- Portfolio value includes long assets and short liabilities correctly.
- Strategy quantities can be upper bounds, but the risk manager remains authoritative.

## Remaining Deployment Blockers

### 1. Durable Send Idempotency

The client order ID reduces accidental duplication, but the send intent and exchange
acknowledgement are not persisted atomically before the HTTP request. A timeout after
CoinDCX accepts an order can leave the local process uncertain whether submission
succeeded.

Required:

1. Persist `pending_send` with client ID and complete order payload.
2. Submit to CoinDCX.
3. Persist the exchange ID and `accepted` state.
4. On restart, reconcile all `strat_*` client IDs before allowing new orders.

### 2. Exchange-Held Protective Orders

Stops, targets, and trailing stops are synthetic and evaluated by the local process.
If the process, network, or host fails, CoinDCX does not hold the protective exit.

Required: use CoinDCX-supported native stop/trigger orders where available, or run a
separately supervised protection service. Do not run unattended while protection
exists only in process memory.

### 3. Stale-Market-Data Guard

A symbol must be blocked when the latest candle is older than the expected timeframe
plus a small tolerance. Data-fetch failure must not reuse old candles as a valid
trading signal.

Required: reject new entries, alert, and optionally flatten risk when data age exceeds
the configured threshold.

### 4. Order Polling Latency

Pending CoinDCX orders are polled sequentially. With multiple orders, fill recognition
latency grows linearly and can exceed the strategy cycle.

Required: bounded concurrent polling or a supported batch/websocket order update path,
while respecting exchange rate limits.

### 5. Emergency Kill Switch

Graceful shutdown cancels tracked pending orders, but a production operator needs one
explicit action that:

1. Blocks new entries.
2. Cancels every tracked exchange order.
3. Reconciles current balances and positions.
4. Flattens configured positions.
5. Persists the halted state.

### 6. CoinDCX Market Rules

Before submission, validate current per-market order support, minimum quantity,
quantity precision, price tick, notional minimum, fee tier, and available INR balance.
Do not rely on static assumptions in config.

## Deployment Gates

A strategy may move from paper to minimum-size live trading only when all are true:

- Positive post-tax performance on an untouched holdout period.
- Positive return under the account's actual CoinDCX fee tier and doubled-slippage stress.
- Maximum drawdown below the approved risk limit.
- Profit factor above 1.2 with enough independent trades to be meaningful.
- Similar behavior across multiple chronological windows, not one optimized interval.
- At least 6-8 weeks of paper execution with no duplicate, missed, or orphan orders.
- Durable idempotency, stale-data protection, native/supervised stops, and kill switch complete.
- Paper fills and CoinDCX-reported fills reconcile within configured tolerances.

## Verification Evidence

- Deterministic Rust integration test: one-unit 100 to 120 trade produces exactly
  `20` P&L, `1020` final equity, and `2%` return.
- The same ledger was reproduced with the public `backtesting.py` framework.
- A zero-cost BTC diagnostic produced `3.81799%`; 258 logged closed-trade P&Ls summed
  to exactly `3.81799%` of initial capital.
- Realistic fees and slippage removed that small edge, showing strategy economics—not
  unexplained ledger drift—caused the negative result.
- Full Rust tests and strict Clippy checks pass.

## Operational Rule

Passing tests does not make a strategy profitable or the live system operationally
safe. Real-money mode remains disabled in practice until every deployment blocker
above is closed and the selected strategy passes all deployment gates.
