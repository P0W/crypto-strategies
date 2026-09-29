# CoinDCX Live Trading Readiness

**Updated:** 2026-09-28

## Current decision

**A broker-neutral real spot execution path and CoinDCX adapter are implemented.**
Paper remains the default. `live --live` performs capability, ownership and balance
checks before submitting orders. Unsupported markets fail closed without a bypass.
Development validation uses an offline broker and loopback HTTP fixtures; no
private account calls or real orders were made during implementation.

The simulated streaming mode is `live --paper`. It consumes public CoinDCX
candles, using the same deterministic `TradingEngine` as backtesting. It is a
closed-candle simulator, not a high-frequency execution system.

## Shared execution design

- One synchronous core handles strategy callbacks, order sizing, pending cash,
  position-slot and portfolio-heat reservations, stops, fills, FIFO lots, fees,
  trade records, equity, and risk state.
- Historical CSV input and async streaming input are adapters. They do not
  duplicate trading decisions or accounting.
- The same `backtest.use_t1_execution` configuration selects next-open market
  execution or close-based market execution in both adapters. Resting orders are
  eligible only on a later bar. Omitting the backtest CLI override preserves the
  configured policy; `--use-t1-execution` explicitly enables next-open execution.
- Entries are resized or cancelled if their executable price would breach risk or
  exposure constraints. Limit affordability uses the limit price without market
  slippage. Synthetic short covers may realize a cash deficit instead of leaving
  an insolvent position open; this does not authorize borrowing or spot shorting.
- Protective stops precede ambiguous intrabar resting reductions, but not orders
  already executable at the open. Opposing pending entries are rejected without
  an explicit OCO/reversal policy.
- Opening reductions and protection run across all symbols before opening
  additions are sized; new positions are then checked for opening protection.
  Intrabar entries use execution-price marks and cannot exit at pre-entry opening
  prices or rely on favorable extremes that may precede entry. Adverse extremes
  remain conservative; the later close is the guaranteed favorable observation.
  Protection runs before final close marks so closed positions do not manufacture
  a later equity peak.
- Candle timestamps identify openings. Higher-timeframe OHLCV is available only
  after that candle closes. Symbols must use the same primary timestamps.
- Trailing stops derived from a close apply prospectively, not to an earlier
  portion of that candle.
- Cumulative execution reports are converted into incremental fills using the
  difference in total notional, not the cumulative average as the new fill price.
  Exit reports accumulate per order, with one trade and win/loss update on completion
  or cancellation; separate grid reduction orders remain separate trades. The last
  4,096 terminal order cursors are persisted in completion order, including cancelled
  partial orders; active reports also retain exact duplicate/conflict metadata.
  Conflicting reports and IDs outside that retained window require reconciliation,
  not blind retry.
- Both crates forbid unsafe code. The former duplicated real-order loop and its
  misleading HFT latency logging have been removed rather than left unreachable.

## Paper operation and recovery

```text
cargo run -- live --config configs/sample_config.json --paper --state-db paper-state.db
```

Each committed bar writes one complete SQLite snapshot using WAL and FULL
synchronization. Snapshot state includes cash, the peak capital and losing streak,
FIFO entry lots and commissions, stops, pending order IDs and remaining quantities,
last processed bar, strategy cooldown/pause state, and component-cost state.
Version 2 also stores pending exit aggregates, signal regime scores, and terminal
execution cursors and their retention order. Older version-1 snapshots are not migrated automatically; use a
new paper database rather than guessing missing reconciliation state.
The JSON export is a backup; SQLite is authoritative. A persistence failure stops
the adapter rather than continuing without durable state.

The supplied database filename is honored. A process-lifetime file lock prevents
two runners from using the same state path concurrently. Configuration identity
excludes API credentials and must match on restart.

Old position/checkpoint/order tables are **not** automatically promoted into a
new snapshot: their historical updates were not atomic and may contain stale open
positions. Keep legacy databases, manually reconcile any real exchange orders,
and start paper mode with a new database.

Input is normalized to oldest-first and unfinished candles are excluded. Failed,
empty, stale, or unsynchronized data prevents processing the entire frame.
Duplicate bars do not execute again. A restart can replay missed **paper** bars
within the 500-bar fetched history. Every expected primary candle must be present
for every symbol before any replay bar is committed, using calendar month boundaries
where applicable. A missing intermediate bar or larger recovery gap stops with an error.
Ctrl+C persists simulated positions and pending orders; it does not flatten them.

## Real spot operation

Prepare a separate JSON configuration with supported spot symbols, a common quote
currency, capital in that currency, and explicitly approved risk limits. Native
metadata, not filename conventions, determines the candle pair and currencies.
The public CoinDCX metadata inspected during implementation advertised only limit
and market orders for BTCINR, ETHINR, SOLINR and BNBINR. BTCUSDT and ETHUSDT advertised
stop-limit support. The adapter checks the current response rather than hardcoding
that list. Renaming a symbol is not a currency conversion or strategy validation.

```text
cargo run -- live --live --preflight --config <spot-config.json> --state-db live-state.db
```

Preflight makes authenticated **read** requests and may update local reconciliation
state; it never submits or cancels an exchange order. It requires matching account/
config identity, supported markets, sufficient unencumbered quote funds, and no
unmanaged holdings/orders in configured assets. A fresh state must start without
holdings in those assets. Existing unrelated assets are neither sold nor imported.
Remove `--preflight` only when authorizing actual trading.

One synchronous core still owns decisions, risk, stops and accounting. Real mode
does not use simulated T+1 or candle fills: it emits intents at the latest closed
candle and waits for actual reports. Quotes drive protection between candle closes.
Missed historical bars are not replayed into new real orders. Spot short entries
are explicitly rejected.

The broker boundary is small: account identity, market rules, balances, quotes,
active-order enumeration, client-ID lookup, submit and cancel. No second broker
adapter, margin trading, or futures implementation is implied.

## Durable execution and protection

- SQLite atomically stores the entire engine and execution journal. Client IDs
  combine a persisted random namespace and engine order ID. The immutable payload
  and send intent commit before HTTP. Mutations have no transport retry.
- A lost create response is resolved through the same client ID. Even a subsequent
  not-found response does not authorize resubmission: the original request might
  still be in flight. Such an unresolved intent requires operator/venue investigation.
  `--resume` cannot bypass it, and deleting state is not a recovery procedure.
- Cancel requests retain engine orders and reservations until a terminal report.
  A cancel acknowledgement means initiation, not completion. A later retry is
  allowed only after lookup confirms that the order remains active.
- Cumulative quantity, average price and fee are validated against the immutable
  request. Duplicate economic reports have no effect. Fees are quote-currency fees;
  fee-only increments adjust cash and original FIFO allocations without fake fills.
  Regressions, price corrections, unknown states or unexplained wallet differences
  halt entries. The order report is not assumed to include every tax/wallet debit.
  Recent terminal reports are polled normally; a wallet discrepancy also triggers
  an audit of retained older reports before halting. Terminal journal records and
  FIFO fee attribution are retained deliberately: deleting them after a short
  timeout would make later real fees unrecoverable. Exact duplicate reports do
  not rewrite the snapshot. Snapshot/history size still grows with completed
  orders and must be monitored; this is not a bounded-memory HFT journal.
- Precision uses decimal arithmetic, quantity is rounded down, and price ticks,
  market quantity limits, minimum notional, balances and capabilities are checked.
  Protection is checked before buying, but an exchange partial fill can still be
  below the minimum protective size. This is an explicit protection failure, not
  a fictitious successful stop or a dust writeoff.
- Confirmed inventory receives a native stop-limit. Additional entries wait for
  acknowledged protection. Partial non-protective orders are cancelled/reconciled;
  only actually filled quantities enter the ledger.
- There is **no assumed atomic spot OCO**. A strategy reduction waits locally until
  executable; outstanding buys are cancelled first, and the native stop is
  cancelled and reconciled before sending the sell. Trailing-stop replacements use
  the same cancel/reconcile sequence. These handoffs have exposure windows.
  A stop-limit can remain unfilled through a gap, and an unavailable process,
  network or venue can prevent its market-exit fallback. This is not guaranteed
  loss containment.

## Halt, restart and account boundaries

Create `live-state.halt` next to `live-state.db` (or the corresponding basename for
a custom state path) to durably halt entries. Reconciliation and protection continue.
Failures also set a durable halt. Remove the file, investigate the cause, and use
`--resume` to clear a halt only after reconciliation and protection checks pass.
There is no force-resume flag.

Ctrl+C requests cancellation of non-protective orders, reconciles them, and attempts
to leave acknowledged native protection on remaining holdings. It does not promise
to flatten the account. If shutdown cannot confirm cancellation/protection, the
command reports failure and the operator must inspect the venue immediately.
Restart reconciles the journal before new decisions. Do not delete the database or
silently migrate a legacy/paper database.

Use one writer for the account. Process-lifetime database and broker-account locks
exclude other local instances, including instances using another API key for the
same account. They do not prevent another machine, manual trading, deposits or
withdrawals. Such activity can halt reconciliation and must not be mixed with the
managed inventory. Credentials are excluded from persisted identity.

Offline validation is necessary, not sufficient for deployment. Supervised
account-specific verification of API permissions, fee currency, fees/tax debits,
partial fills and native stops remains an operational gate before unattended use.

API references: [CoinDCX documentation](https://docs.coindcx.com/),
[public market metadata](https://api.coindcx.com/exchange/v1/markets_details), and
[spot order help](https://coindcx.com/api/help/Placing%20Orders%20using%20CoinDCX%20API/Spot%20Order).

## Economic deployment gates

Regenerate historical results after the temporal and execution corrections.
Previously published performance numbers and candidate holdouts are not current
evidence. Require untouched chronological validation, actual account fees,
slippage stress, approved drawdown limits, and adequate independent trades.
Code correctness and paper parity do not establish profitability.
Insolvency preserves negative equity and uncapped losses. Calmar is explicitly
undefined for nonpositive terminal equity (`N/A` in the CLI, negative infinity
internally); undefined optimization scores rank last instead of comparing equal
to viable candidates.
Nonpositive or invalid terminal equity also makes a candidate ineligible under
every other optimization objective, including win rate. Such candidates remain
visible in results but cannot overwrite a configuration. Saved optimization
metadata without verified positive terminal equity is re-evaluated rather than
trusted as a selection baseline.
