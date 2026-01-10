# Architecture

This document provides a visual overview of the system architecture.

## System Overview

```mermaid
flowchart TB
    subgraph CLI["CLI Commands"]
        backtest["backtest"]
        optimize["optimize"]
        live["live"]
        download["download"]
    end

    subgraph Core["Core Engine"]
        Backtester
        Optimizer
        RiskManager
    end

    subgraph OMS["Order Management System"]
        OrderBook
        ExecutionEngine
        PositionManager
    end

    subgraph Strategies["Strategy Layer"]
        StrategyTrait["Strategy Trait"]
        VolatilityRegime["volatility_regime"]
        RegimeGrid["regime_grid"]
        MomentumScalper["momentum_scalper"]
        QuickFlip["quick_flip"]
        RangeBreakout["range_breakout"]
    end

    subgraph Persistence["Persistence"]
        StateManager["SqliteStateManager"]
        SQLite[(SQLite DB)]
        JSON[(JSON Backup)]
    end

    subgraph Exchange["Exchange Clients"]
        CoinDCX
        Zerodha
        Binance["Binance (data only)"]
    end

    backtest --> Backtester
    optimize --> Optimizer
    live --> Backtester
    download --> Binance

    Optimizer -->|parallel backtests| Backtester
    Backtester --> RiskManager
    Backtester --> OMS
    Backtester --> StrategyTrait

    StrategyTrait -.->|implements| VolatilityRegime
    StrategyTrait -.->|implements| RegimeGrid
    StrategyTrait -.->|implements| MomentumScalper
    StrategyTrait -.->|implements| QuickFlip
    StrategyTrait -.->|implements| RangeBreakout

    live --> StateManager
    StateManager --> SQLite
    StateManager --> JSON
    live --> CoinDCX
    live --> Zerodha
```

## Backtester Flow

```mermaid
flowchart TD
    Start([Start]) --> LoadData["Load OHLCV Data"]
    LoadData --> AlignData["Align Multi-Symbol Data"]
    AlignData --> InitComponents["Initialize Components"]
    
    subgraph Init["Initialization"]
        InitComponents --> CreateStrategy["Create Strategy per Symbol"]
        CreateStrategy --> InitRisk["Init RiskManager"]
        InitRisk --> InitOMS["Init OMS Components"]
    end

    InitOMS --> BarLoop{"For Each Bar"}
    
    subgraph BarProcessing["Bar Processing (3 Phases)"]
        BarLoop -->|Phase 0| ExecuteQueued["Execute T+1 Queued Orders"]
        ExecuteQueued -->|Phase 1| ProcessFills["Process Pending Order Fills"]
        ProcessFills -->|Phase 2| GenerateOrders["Generate New Orders"]
    end

    GenerateOrders --> UpdateEquity["Update Equity Curve"]
    UpdateEquity --> BarLoop
    
    BarLoop -->|End of Data| ClosePositions["Close Remaining Positions"]
    ClosePositions --> CalcMetrics["Calculate Performance Metrics"]
    CalcMetrics --> Return([Return BacktestResult])
```

## Order Management System (OMS)

```mermaid
flowchart LR
    subgraph Strategy
        GenOrders["generate_orders"]
    end

    subgraph OrderFlow["Order Flow"]
        OrderRequest["OrderRequest"]
        Order["Order"]
        OrderBook["OrderBook"]
    end

    subgraph Execution
        ExecEngine["ExecutionEngine"]
        CheckFill["check_fill"]
        ExecFill["execute_fill"]
        Fill["Fill"]
    end

    subgraph Positions
        PosMgr["PositionManager"]
        Position["Position"]
        Trade["Trade"]
    end

    GenOrders -->|creates| OrderRequest
    OrderRequest -->|into_order| Order
    Order -->|add_order| OrderBook
    OrderBook -->|get_fillable_orders| ExecEngine
    ExecEngine --> CheckFill
    CheckFill -->|candle OHLC match| ExecFill
    ExecFill -->|creates| Fill
    Fill -->|add_fill| PosMgr
    PosMgr -->|FIFO accounting| Position
    Position -->|on close| Trade
```

## Risk Manager

```mermaid
flowchart TD
    subgraph Inputs
        Capital["Current Capital"]
        Peak["Peak Capital"]
        Positions["Open Positions"]
        Config["Risk Config"]
    end

    subgraph Checks["Risk Checks"]
        Drawdown["Calculate Drawdown"]
        Halt{"Halt Trading?"}
        PortfolioHeat["Check Portfolio Heat"]
    end

    subgraph Sizing["Position Sizing"]
        BaseRisk["Base Risk Amount"]
        RegimeAdj["Regime Adjustment"]
        DrawdownMult["Drawdown Multiplier"]
        LossMult["Consecutive Loss Multiplier"]
        HeatLimit["Heat Limit Adjustment"]
        FinalSize["Final Position Size"]
    end

    Capital --> Drawdown
    Peak --> Drawdown
    Drawdown --> Halt
    Halt -->|Yes: DD >= 20%| Block([Block All Trades])
    Halt -->|No| BaseRisk

    Config --> BaseRisk
    BaseRisk --> RegimeAdj
    RegimeAdj --> DrawdownMult
    DrawdownMult --> LossMult
    Positions --> PortfolioHeat
    PortfolioHeat --> HeatLimit
    LossMult --> HeatLimit
    HeatLimit --> FinalSize
```

## Strategy Trait

```mermaid
classDiagram
    class Strategy {
        <<trait>>
        +name() str
        +clone_boxed() Box
        +generate_orders(ctx) Vec
        +calculate_stop_loss(candles, entry_price) f64
        +calculate_take_profit(candles, entry_price) f64
        +update_trailing_stop(position, price, candles) Option
        +required_timeframes() Vec
        +get_regime_score(candles) f64
        +on_bar(ctx)
        +on_order_filled(fill, position)
        +on_trade_closed(trade)
        +init()
    }

    class StrategyContext {
        +symbol: String
        +candles: Vec
        +position: Option
        +pending_orders: Vec
        +cash: f64
        +portfolio_value: f64
    }

    class OrderRequest {
        +symbol: String
        +side: OrderSide
        +order_type: OrderType
        +quantity: Money
        +limit_price: Option
        +stop_price: Option
    }

    Strategy ..> StrategyContext : uses
    Strategy ..> OrderRequest : creates
```

## Optimizer Flow

```mermaid
flowchart TD
    Start([Start]) --> LoadConfig["Load Config with Grid"]
    LoadConfig --> GenCombinations["Generate Parameter Combinations"]
    
    subgraph GridExpansion["Grid Expansion"]
        GenCombinations --> CartesianProduct["Cartesian Product"]
        CartesianProduct --> Combinations["N Parameter Combinations"]
    end

    Combinations --> ParallelExec{"Parallel Execution"}
    
    subgraph RayonPool["Rayon Thread Pool"]
        ParallelExec -->|par_iter| Worker1["Worker 1"]
        ParallelExec -->|par_iter| Worker2["Worker 2"]
        ParallelExec -->|par_iter| WorkerN["Worker N"]
        Worker1 --> Backtest1["Run Backtest"]
        Worker2 --> Backtest2["Run Backtest"]
        WorkerN --> BacktestN["Run Backtest"]
    end

    Backtest1 --> Collect["Collect Results"]
    Backtest2 --> Collect
    BacktestN --> Collect

    Collect --> Sort["Sort by Metric"]
    Sort --> Compare{"Best > Baseline + ε?"}
    Compare -->|Yes| UpdateConfig["Update Config File"]
    Compare -->|No| KeepConfig["Keep Current Config"]
    UpdateConfig --> Display["Display Top N Results"]
    KeepConfig --> Display
    Display --> End([End])
```

## State Manager (Live Trading)

```mermaid
flowchart TD
    subgraph LiveTrading["Live Trading Engine"]
        Engine["Trading Engine"]
    end

    subgraph StateManager["SqliteStateManager"]
        subgraph PositionOps["Position Operations"]
            SavePos["save_position"]
            LoadPos["load_positions"]
            GetPos["get_position"]
        end
        subgraph CheckpointOps["Checkpoint Operations"]
            SaveChk["save_checkpoint"]
            LoadChk["load_checkpoint"]
        end
        subgraph TradeOps["Trade Operations"]
            RecordTrade["record_trade"]
        end
        subgraph BackupOps["Backup"]
            ExportJSON["export_json"]
        end
    end

    subgraph SQLite["SQLite Database (WAL mode)"]
        Positions[("positions
        - symbol PK
        - side, quantity
        - entry_price, entry_time
        - stop_loss, take_profit
        - status, order_id
        - pnl, exit_price, exit_time
        - metadata")]
        Checkpoints[("checkpoints
        - timestamp, cycle_count
        - portfolio_value, cash
        - open_positions
        - drawdown_pct
        - consecutive_losses
        - config_hash")]
        Trades[("trades
        - symbol, side, quantity
        - entry/exit price/time
        - gross_pnl, fees, tax, net_pnl
        - exit_reason, strategy_signal
        - atr_at_entry, stop_loss
        - risk_reward_actual")]
    end

    subgraph Backup["JSON Backup"]
        JSONFile[("state_backup.json")]
    end

    Engine -->|open/update position| SavePos
    Engine -->|query all positions| LoadPos
    Engine -->|query single symbol| GetPos
    Engine -->|periodic snapshot| SaveChk
    Engine -->|crash recovery| LoadChk
    Engine -->|completed trade| RecordTrade
    Engine -->|auto backup| ExportJSON

    SavePos --> Positions
    LoadPos --> Positions
    GetPos --> Positions
    SaveChk --> Checkpoints
    LoadChk --> Checkpoints
    RecordTrade --> Trades
    ExportJSON --> JSONFile
```

## Data Flow Summary

```mermaid
flowchart LR
    subgraph Input
        CSV[(OHLCV CSV)]
        Config[(Config JSON)]
    end

    subgraph Processing
        Backtester
        Strategy
        OMS
        Risk[RiskManager]
    end

    subgraph Output
        Metrics["PerformanceMetrics"]
        Trades["Trade History"]
        Equity["Equity Curve"]
    end

    CSV --> Backtester
    Config --> Backtester
    Backtester <--> Strategy
    Backtester <--> OMS
    Backtester <--> Risk
    Strategy -->|OrderRequest| OMS
    OMS -->|Fill| Strategy
    Risk -->|position size| Backtester
    Backtester --> Metrics
    Backtester --> Trades
    Backtester --> Equity
```

## Key Types

| Component | Key Types |
|-----------|-----------|
| **OMS** | `Order`, `OrderRequest`, `Fill`, `Position`, `OrderBook`, `ExecutionEngine`, `PositionManager` |
| **Strategy** | `Strategy` (trait), `StrategyContext`, `Candle`, `Signal` |
| **Risk** | `RiskManager`, `RiskConfig` |
| **Backtest** | `Backtester`, `BacktestResult`, `PerformanceMetrics`, `Trade` |
| **Optimizer** | `OptimizationResult`, `GridConfig` |
| **State** | `SqliteStateManager`, `PortfolioCheckpoint` |
| **Types** | `Money` (decimal wrapper), `Symbol`, `Timeframe` |
