# Order Management System (OMS) Design Document

## Executive Summary

This document outlines the design for a production-grade Order Management System (OMS) for the crypto-strategies backtesting engine. The OMS will enable strategies to place limit orders, manage multiple positions per symbol, and execute partial fills when price levels are touched—critical capabilities for grid trading and other sophisticated strategies.

**Design Philosophy**: Ultra-low latency, zero-copy where possible, cache-friendly data structures, production-ready with comprehensive error handling.

---

## 1. Current Limitations

### 1.1 Existing Architecture
```rust
// Current: One position per symbol
positions: HashMap<Symbol, Position>

// Current: Immediate execution at next candle open (T+1)
pending_orders: HashMap<Symbol, PendingOrder>
```

**Problems**:
- Cannot place multiple buy/sell orders at different price levels
- No intra-candle execution (misses partial fills)
- No order lifecycle (place → modify → cancel → fill)
- Grid strategies can't simulate true market-making behavior

### 1.2 What Grid Trading Needs
```
Example: BTC at $100,000

Grid Buy Orders (limit orders waiting to fill):
├─ $99,000 ─ 0.01 BTC
├─ $98,000 ─ 0.01 BTC  
├─ $97,000 ─ 0.01 BTC
└─ $96,000 ─ 0.01 BTC

Grid Sell Orders (limit orders waiting to fill):
├─ $101,000 ─ 0.01 BTC
├─ $102,000 ─ 0.01 BTC
├─ $103,000 ─ 0.01 BTC
└─ $104,000 ─ 0.01 BTC

As price moves through levels, orders fill and new ones are placed
```

---

## 2. Architecture Overview

### 2.1 High-Level Components

```
┌─────────────────────────────────────────────────────────┐
│                      Strategy                            │
│  (generates order requests: buy/sell at price X)        │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│                  Order Manager                           │
│  • Validates orders against risk limits                 │
│  • Manages order lifecycle (New → Open → Filled)        │
│  • Routes to execution engine                           │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│                 Execution Engine                         │
│  • Matches orders against OHLC candles                  │
│  • Handles partial fills                                │
│  • Updates positions and P&L                            │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│                 Position Manager                         │
│  • Tracks multiple positions per symbol                 │
│  • Calculates unrealized P&L                            │
│  • Manages position netting                             │
└─────────────────────────────────────────────────────────┘
```

### 2.2 Data Flow

```
Backtest Loop:
1. Candle arrives (e.g., BTC at $99,500)
2. Execution Engine checks all pending orders
3. Orders with price ≤ $99,500 (buy side) fill
4. Position Manager updates positions
5. Strategy receives fill notifications
6. Strategy may place new orders
7. Risk Manager validates new orders
8. Orders added to OrderBook
```

---

## 3. Core Data Structures

### 3.1 Order Types

```rust
/// Order type - determines execution logic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Execute immediately at market price (next candle open)
    Market,
    
    /// Execute when price reaches limit price
    /// Buy limit: executes when price ≤ limit_price
    /// Sell limit: executes when price ≥ limit_price
    Limit,
    
    /// Stop-loss: converts to market when stop triggered
    /// Buy stop: triggers when price ≥ stop_price
    /// Sell stop: triggers when price ≤ stop_price
    Stop,
    
    /// Stop-limit: converts to limit order when stop triggered
    StopLimit,
}

/// Time-in-force specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good till cancelled
    GTC,
    
    /// Good till date
    GTD(DateTime<Utc>),
    
    /// Immediate or cancel (fill immediately or cancel)
    IOC,
    
    /// Fill or kill (fill completely or cancel)
    FOK,
}

/// Order state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderState {
    /// Order created, not yet submitted
    Pending,
    
    /// Order submitted to exchange/orderbook
    Submitted,
    
    /// Order accepted and active
    Open,
    
    /// Order partially filled
    PartiallyFilled,
    
    /// Order completely filled
    Filled,
    
    /// Order cancelled by user
    Cancelled,
    
    /// Order rejected (insufficient margin, invalid price, etc.)
    Rejected,
    
    /// Order expired (GTD timeout)
    Expired,
}

/// Core order structure (optimized for cache efficiency)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(C)]  // Cache-friendly layout
pub struct Order {
    /// Unique order ID (use u64 for performance)
    pub id: OrderId,
    
    /// Symbol being traded
    pub symbol: Symbol,
    
    /// Order side (Buy/Sell)
    pub side: Side,
    
    /// Order type
    pub order_type: OrderType,
    
    /// Limit price (for limit/stop-limit orders)
    pub limit_price: Option<f64>,
    
    /// Stop price (for stop/stop-limit orders)
    pub stop_price: Option<f64>,
    
    /// Total order quantity
    pub quantity: f64,
    
    /// Filled quantity so far
    pub filled_quantity: f64,
    
    /// Remaining quantity
    pub remaining_quantity: f64,
    
    /// Average fill price
    pub average_fill_price: f64,
    
    /// Current state
    pub state: OrderState,
    
    /// Time in force
    pub time_in_force: TimeInForce,
    
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    
    /// Strategy tag (for multi-strategy portfolios)
    pub strategy_tag: Option<String>,
    
    /// Client order ID (optional, for strategy tracking)
    pub client_id: Option<String>,
}

/// Order ID type (use u64 for performance)
pub type OrderId = u64;
```

### 3.2 Position Tracking

```rust
/// Enhanced position supporting multiple entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Symbol
    pub symbol: Symbol,
    
    /// Net position side (aggregated)
    pub side: Side,
    
    /// Average entry price (FIFO-weighted)
    pub average_entry_price: f64,
    
    /// Total quantity
    pub quantity: f64,
    
    /// Realized P&L (closed trades)
    pub realized_pnl: f64,
    
    /// Unrealized P&L (open position)
    pub unrealized_pnl: f64,
    
    /// Individual fills comprising this position
    pub fills: Vec<Fill>,
    
    /// First entry time
    pub first_entry_time: DateTime<Utc>,
    
    /// Last update time
    pub last_update_time: DateTime<Utc>,
}

/// Individual fill record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    /// Order ID that generated this fill
    pub order_id: OrderId,
    
    /// Fill price
    pub price: f64,
    
    /// Fill quantity
    pub quantity: f64,
    
    /// Fill timestamp
    pub timestamp: DateTime<Utc>,
    
    /// Commission paid
    pub commission: f64,
    
    /// Liquidity flag (maker/taker)
    pub is_maker: bool,
}
```

### 3.3 Order Book

```rust
/// Ultra-fast order book using BTreeMap for price-time priority
pub struct OrderBook {
    /// Buy orders sorted by price (descending) - best bid first
    buy_orders: BTreeMap<OrderedFloat<f64>, VecDeque<OrderId>>,
    
    /// Sell orders sorted by price (ascending) - best ask first  
    sell_orders: BTreeMap<OrderedFloat<f64>, VecDeque<OrderId>>,
    
    /// Fast lookup: OrderId → Order
    orders: HashMap<OrderId, Order>,
    
    /// Symbol for this order book
    symbol: Symbol,
}

impl OrderBook {
    /// Add order with price-time priority
    pub fn add_order(&mut self, order: Order) {
        // O(log n) insertion
    }
    
    /// Cancel order
    pub fn cancel_order(&mut self, order_id: OrderId) -> Option<Order> {
        // O(log n) removal
    }
    
    /// Get orders that would fill at given price
    /// Returns: Vec<OrderId> sorted by priority
    pub fn get_fillable_orders(&self, price: f64, side: Side) -> Vec<OrderId> {
        // O(k) where k = number of fillable orders
    }
    
    /// Get best bid price
    pub fn best_bid(&self) -> Option<f64> {
        // O(1)
    }
    
    /// Get best ask price
    pub fn best_ask(&self) -> Option<f64> {
        // O(1)
    }
}
```

---

## 4. Execution Logic

### 4.1 Intra-Candle Fill Detection

```rust
/// Check if order fills during this candle
fn check_fill(order: &Order, candle: &Candle) -> Option<FillPrice> {
    match (order.side, order.order_type) {
        // Buy limit: fills if candle low ≤ limit price
        (Side::Buy, OrderType::Limit) => {
            if candle.low <= order.limit_price.unwrap() {
                // Conservative: use limit price (assumes we're limit maker)
                Some(FillPrice {
                    price: order.limit_price.unwrap(),
                    is_maker: true,
                })
            } else {
                None
            }
        }
        
        // Sell limit: fills if candle high ≥ limit price
        (Side::Sell, OrderType::Limit) => {
            if candle.high >= order.limit_price.unwrap() {
                Some(FillPrice {
                    price: order.limit_price.unwrap(),
                    is_maker: true,
                })
            } else {
                None
            }
        }
        
        // Buy stop: triggers if candle high ≥ stop price
        (Side::Buy, OrderType::Stop) => {
            if candle.high >= order.stop_price.unwrap() {
                // Becomes market order, fills at stop price + slippage
                Some(FillPrice {
                    price: order.stop_price.unwrap() * (1.0 + slippage),
                    is_maker: false,
                })
            } else {
                None
            }
        }
        
        // Sell stop: triggers if candle low ≤ stop price
        (Side::Sell, OrderType::Stop) => {
            if candle.low <= order.stop_price.unwrap() {
                Some(FillPrice {
                    price: order.stop_price.unwrap() * (1.0 - slippage),
                    is_maker: false,
                })
            } else {
                None
            }
        }
        
        // Market orders: fill at next candle open
        (_, OrderType::Market) => {
            Some(FillPrice {
                price: candle.open,
                is_maker: false,
            })
        }
        
        _ => None,
    }
}
```

### 4.2 Fill Priority

When multiple orders fill at same price level:
1. **Price-time priority**: Earlier orders fill first
2. **Pro-rata for large fills**: When simulating exchange behavior
3. **FIFO within same price**: Maintained by VecDeque in OrderBook

### 4.3 Partial Fill Logic

```rust
fn execute_partial_fill(
    order: &mut Order,
    fill_price: f64,
    max_fill_qty: f64,
) -> Fill {
    let fill_qty = f64::min(order.remaining_quantity, max_fill_qty);
    
    order.filled_quantity += fill_qty;
    order.remaining_quantity -= fill_qty;
    
    // Update average fill price
    let total_value = order.average_fill_price * order.filled_quantity;
    let new_value = fill_price * fill_qty;
    order.average_fill_price = (total_value + new_value) 
        / (order.filled_quantity + fill_qty);
    
    // Update state
    order.state = if order.remaining_quantity == 0.0 {
        OrderState::Filled
    } else {
        OrderState::PartiallyFilled
    };
    
    Fill {
        order_id: order.id,
        price: fill_price,
        quantity: fill_qty,
        timestamp: Utc::now(),
        commission: calculate_commission(fill_qty, fill_price),
        is_maker: order.order_type == OrderType::Limit,
    }
}
```

---

## 5. Strategy Interface Changes

### 5.1 New Strategy Trait

```rust
pub trait Strategy: Send + Sync {
    // Existing methods remain unchanged for backward compatibility
    fn generate_signal(&self, ...) -> Signal { ... }
    
    // NEW: Advanced order placement (opt-in)
    fn generate_orders(&self, context: &StrategyContext) -> Vec<OrderRequest> {
        // Default implementation: convert Signal to single market order
        vec![]
    }
    
    // NEW: Order fill notification
    fn on_order_filled(&mut self, fill: &Fill, position: &Position) {
        // Default: no-op
    }
    
    // NEW: Order cancelled notification
    fn on_order_cancelled(&mut self, order: &Order) {
        // Default: no-op
    }
}

/// Context provided to strategy for decision-making
pub struct StrategyContext<'a> {
    pub symbol: &'a Symbol,
    pub candles: &'a [Candle],
    pub current_position: Option<&'a Position>,
    pub open_orders: &'a [Order],
    pub cash_available: f64,
    pub equity: f64,
}

/// Order request from strategy
#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub time_in_force: TimeInForce,
    pub client_id: Option<String>,
}
```

### 5.2 Grid Strategy Example

```rust
impl Strategy for RegimeGridStrategy {
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = vec![];
        
        // Cancel orders too far from current price
        for order in ctx.open_orders {
            if should_cancel(order, ctx.candles.last().unwrap()) {
                orders.push(OrderRequest::cancel(order.id));
            }
        }
        
        // Place new grid orders
        let current_price = ctx.candles.last().unwrap().close;
        
        for i in 1..=self.config.max_grids {
            let buy_price = current_price * (1.0 - i as f64 * self.config.grid_spacing_pct);
            let sell_price = current_price * (1.0 + i as f64 * self.config.grid_spacing_pct);
            
            // Only place if not already have order near that price
            if !has_order_near_price(ctx.open_orders, buy_price) {
                orders.push(OrderRequest {
                    symbol: ctx.symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    quantity: self.config.order_size,
                    limit_price: Some(buy_price),
                    stop_price: None,
                    time_in_force: TimeInForce::GTC,
                    client_id: Some(format!("grid_buy_{}", i)),
                });
            }
            
            if !has_order_near_price(ctx.open_orders, sell_price) {
                orders.push(OrderRequest {
                    symbol: ctx.symbol.clone(),
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    quantity: self.config.order_size,
                    limit_price: Some(sell_price),
                    stop_price: None,
                    time_in_force: TimeInForce::GTC,
                    client_id: Some(format!("grid_sell_{}", i)),
                });
            }
        }
        
        orders
    }
    
    fn on_order_filled(&mut self, fill: &Fill, position: &Position) {
        // Grid filled - may want to place opposite order
        tracing::info!("Grid order filled: {:?}", fill);
    }
}
```

---

## 6. Performance Optimizations

### 6.1 Memory Management

```rust
// Pre-allocate collections with capacity
let mut orders = HashMap::with_capacity(1000);
let mut fills = Vec::with_capacity(10000);

// Use SmallVec for small collections (stack allocation)
use smallvec::SmallVec;
type FillVec = SmallVec<[Fill; 4]>;  // Most orders have < 4 fills

// Use arena allocation for temporary objects
use bumpalo::Bump;
let arena = Bump::new();
```

### 6.2 Cache Optimization

```rust
// Pack hot data together
#[repr(C)]
struct OrderHotData {
    id: u64,                    // 8 bytes
    price: f64,                 // 8 bytes
    quantity: f64,              // 8 bytes
    state: OrderState,          // 1 byte
    // Total: 25 bytes (fits in cache line)
}

// Keep cold data separate
struct OrderColdData {
    created_at: DateTime<Utc>,
    client_id: Option<String>,
    // Rarely accessed
}
```

### 6.3 Parallel Execution

```rust
// Process symbols in parallel
use rayon::prelude::*;

symbols.par_iter().for_each(|symbol| {
    // Each symbol has independent orderbook
    // No locks needed - embarrassingly parallel
    process_symbol_orders(symbol);
});
```

### 6.4 Fast Order ID Generation

```rust
// Atomic counter (no lock overhead)
static ORDER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_order_id() -> OrderId {
    ORDER_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}
```

---

## 7. Backward Compatibility

### 7.1 Migration Path

**Phase 1: Dual Mode**
```rust
// Strategies can use old or new interface
if strategy.supports_advanced_orders() {
    let orders = strategy.generate_orders(ctx);
    order_manager.submit_orders(orders);
} else {
    // Fallback to old signal-based approach
    let signal = strategy.generate_signal(...);
    convert_signal_to_market_order(signal);
}
```

**Phase 2: Default Implementations**
```rust
// Old strategies automatically work via default impl
impl Strategy for OldStrategy {
    // generate_orders() has default implementation
    // Converts generate_signal() to OrderRequest
}
```

### 7.2 Testing Strategy

1. **Unit tests**: Test OrderBook operations in isolation
2. **Integration tests**: Verify existing strategies produce same results
3. **Regression tests**: Compare old backtest vs new backtest outputs
4. **Performance tests**: Ensure no slowdown for simple strategies

---

## 8. Implementation Phases

### Phase 1: Core Infrastructure (Week 1)
- [ ] Define all types (Order, Fill, OrderBook, etc.)
- [ ] Implement OrderBook with BTreeMap
- [ ] Add unit tests for order matching
- [ ] Benchmark OrderBook performance

### Phase 2: Execution Engine (Week 1-2)
- [ ] Implement fill detection logic
- [ ] Add partial fill support
- [ ] Integrate with backtest loop
- [ ] Test with mock data

### Phase 3: Position Management (Week 2)
- [ ] Enhance Position to track multiple fills
- [ ] Add P&L calculation with FIFO
- [ ] Implement position netting
- [ ] Test edge cases

### Phase 4: Strategy Interface (Week 2-3)
- [ ] Add new Strategy trait methods
- [ ] Provide default implementations
- [ ] Update existing strategies (optional)
- [ ] Migration guide

### Phase 5: Testing & Validation (Week 3)
- [ ] Verify volatility_regime strategy unchanged
- [ ] Test regime_grid with real grid orders
- [ ] Performance benchmarks
- [ ] Documentation

### Phase 6: Optimization (Week 4)
- [ ] Profile hot paths
- [ ] Optimize allocations
- [ ] Parallel execution where possible
- [ ] Final performance tuning

---

## 9. Risk & Mitigation

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Performance regression | High | Medium | Benchmark each commit, use criterion.rs |
| Breaking existing strategies | High | Low | Extensive regression tests, dual mode |
| Complex bugs in fill logic | Medium | Medium | Comprehensive unit tests, fuzzing |
| Memory leaks | Medium | Low | Valgrind, MIRI, careful review |
| Incorrect P&L calculation | High | Low | Test against known results, audit |

---

## 10. Success Metrics

### Performance Targets
- **OrderBook insert**: < 100ns (target: 50ns)
- **Fill detection**: < 1μs per order per candle
- **Backtest slowdown**: < 20% vs current implementation
- **Memory overhead**: < 50MB for 10,000 orders

### Functional Targets
- ✅ Grid strategy shows improved Sharpe ratio
- ✅ Existing strategies produce identical results
- ✅ Zero data races (verified by MIRI)
- ✅ 100% test coverage for core logic

---

## 11. API Examples

### 11.1 Simple Market Order

```rust
// Old way (still works)
fn generate_signal(...) -> Signal {
    Signal::Long
}

// New way (more control)
fn generate_orders(ctx: &StrategyContext) -> Vec<OrderRequest> {
    vec![OrderRequest::market_buy(ctx.symbol.clone(), 1.0)]
}
```

### 11.2 Grid Trading

```rust
fn generate_orders(ctx: &StrategyContext) -> Vec<OrderRequest> {
    let mut orders = vec![];
    let price = ctx.candles.last().unwrap().close;
    
    // Place 10 buy limits below market
    for i in 1..=10 {
        orders.push(OrderRequest::limit_buy(
            ctx.symbol.clone(),
            0.01,  // 0.01 BTC each
            price * (1.0 - 0.01 * i as f64),  // 1% apart
        ));
    }
    
    orders
}
```

### 11.3 Stop Loss + Take Profit

```rust
fn on_order_filled(&mut self, fill: &Fill, position: &Position) {
    // Entry filled - place exit orders
    if fill.order_id == self.entry_order_id {
        let stop_price = fill.price * 0.98;  // 2% stop
        let target_price = fill.price * 1.04; // 4% target
        
        self.exit_orders = vec![
            OrderRequest::stop_sell(symbol, qty, stop_price),
            OrderRequest::limit_sell(symbol, qty, target_price),
        ];
    }
}
```

---

## 12. Appendix: References

### Industry Standards
- **FIX Protocol**: Financial Information eXchange (order message format)
- **ITCH**: Nasdaq TotalView-ITCH protocol (order book updates)
- **FAST**: FIX Adapted for Streaming (low-latency encoding)

### Performance Benchmarks
- **Interactive Brokers**: ~50μs order latency
- **Jump Trading**: < 1μs (co-located)
- **Target**: < 10μs for full order lifecycle in backtest

### Data Structures
- **BTreeMap**: O(log n) operations, cache-friendly for iteration
- **HashMap**: O(1) average, used for ID lookups
- **VecDeque**: O(1) push/pop both ends, FIFO queue

---

## Conclusion

This OMS design provides a solid foundation for production-grade backtesting with limit orders, partial fills, and multi-position tracking. The phased implementation approach minimizes risk while delivering incremental value. The design prioritizes performance, correctness, and backward compatibility.

**Next Steps**:
1. Review this design doc with stakeholders
2. Create GitHub issues for each phase
3. Set up performance benchmarking framework
4. Begin Phase 1 implementation

**Estimated Timeline**: 3-4 weeks for full implementation and testing.

---

*Document Version: 1.0*  
*Author: Copilot (AI Assistant)*  
*Date: 2026-01-05*  
*Status: DRAFT - Awaiting Review*
