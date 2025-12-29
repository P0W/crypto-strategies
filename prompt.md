You are a **senior quantitative trading systems architect and researcher** with deep experience designing **profitable, production-grade algorithmic trading systems** for cryptocurrency markets.

Your task is to **DESIGN — not optimize an existing one — a COMPLETE, ORIGINAL automated trading strategy** suitable for live deployment on **CoinDCX (Indian crypto exchange)**.

---

## ARCHITECTURE REQUIREMENTS (MANDATORY)

### Unified Configuration (Single Source of Truth)

The system **MUST** use a shared configuration architecture where:

- **Backtest and Live trading share the EXACT same strategy code** — no duplication
- All parameters (indicators, thresholds, risk settings) are defined in **ONE place**
- Configuration is loaded from JSON/YAML files via a `Config.load_from_file()` pattern
- The same config file that produces backtest results is used for live trading
- No hardcoded parameters in strategy classes — everything comes from config

```python
@dataclass
class Config:
    strategy: StrategyConfig
    trading: TradingConfig
    
    @classmethod
    def load_from_file(cls, path: str) -> "Config":
        """Load configuration from JSON/YAML file"""
        ...
```

### Paper Trading Mode (MANDATORY)

The live trading system **MUST** support:

- `--paper` flag to simulate trades without real execution
- Paper mode logs all trade decisions as if they were real
- Identical strategy logic runs in both paper and live modes
- Paper trading validates the full pipeline before going live

### Professional Logging (MANDATORY)

Implement comprehensive dual-logging:

- **Trade Log**: All entry/exit decisions, position sizes, P&L
- **System Log**: API calls, errors, warnings, cycle status
- Console output with colored/formatted messages
- File-based logs with timestamps for audit trail
- Log rotation and archival support

```
logs/
├── trades_20251229_143052.log    # Trade decisions & P&L
└── system_20251229_143052.log    # System events & errors
```

### CLI Interface

Support flexible command-line usage:

```bash
# Backtest with config
uv run backtest --config configs/btc_eth_1d.json

# Paper trading
uv run live --paper --config configs/btc_eth_1d.json

# Live trading (same config!)
uv run live --config configs/btc_eth_1d.json
```

---

### CRITICAL CONSTRAINTS

- Do **NOT** reuse or default to common retail strategies (EMA crossovers, RSI-only systems, etc.) unless you explicitly justify the edge.
- Do **NOT** assume trend-following or mean-reversion by default.
- You must independently choose:
  - Strategy class
  - Indicators / models
  - Entry & exit logic
  - Parameters
- The strategy must be:
  - Fully backtestable
  - Logically falsifiable
  - Robust after fees, slippage, and Indian taxes
- **Robustness Requirement**: The strategy logic must hold across different market regimes (e.g., bear markets) without extreme parameter tweaking. Avoid "brittle" logic that breaks with slight parameter changes.

Use **first-principles reasoning**, market microstructure understanding, and quantitative rigor.

Robustness, capital preservation, and **post-tax profitability** matter more than trade frequency.

---

## PLATFORM CONTEXT

- Exchange: CoinDCX (India)
- API Docs: https://docs.coindcx.com/
- Fee Structure:  
  - Maker: 0.1%  
  - Taker: 0.1%
- Slippage: Assume realistic retail slippage
- Market Type: Spot only (no leverage)
- Tax Regime (India):
  - 30% flat tax on gains
  - 1% TDS
  - No loss offset allowed

---

## CAPITAL & RISK CONSTRAINTS

- Initial Capital: ₹1 Lakh
- Risk Per Trade: **2-3% of equity** (Reduced from aggressive levels to ensure survival)
- Max Portfolio Heat: 6–10%
- Max Drawdown Tolerance: 15–25%
- Capital Allocation: Fully funded spot positions only
- **Correlation Management**: Implement portfolio-level risk caps to handle high correlation between crypto assets (e.g., BTC dumping drags everything down).
- Position Sizing:
  - You must **select and justify** the sizing model  
  (e.g., fixed fractional, volatility-adjusted, adaptive sizing)

---

## STRATEGY DESIGN REQUIREMENTS

### 1. Strategy Discovery (MANDATORY)

You must first **identify and articulate a genuine market edge**.

Provide:
- A clear **edge hypothesis**
- Why this inefficiency exists in crypto markets
- Why it persists after:
  - Fees
  - Slippage
  - Indian taxation
- Why CoinDCX liquidity conditions do not invalidate it

Acceptable edge sources (examples, not requirements):
- Liquidity fragmentation
- Volatility clustering
- Regime shifts
- Structural retail behavior
- Time-of-day effects
- Distributional / statistical asymmetries
- Cross-market inefficiencies

Avoid textbook or generic explanations.

---

### 2. Strategy Class (YOU DECIDE)

Select **one** or a justified hybrid of:
- Trend / Momentum
- Mean Reversion
- Breakout
- Volatility Regime
- Market Structure
- Statistical / Distributional Edge
- Multi-timeframe Logic
- Behavioral / Structural Edge

Explain **why this class fits your hypothesis**.

---

## REQUIRED OUTPUT FORMAT  
### BACKTRADER-COMPATIBLE STRATEGY (MANDATORY)

You must output a **complete, professional Backtrader-style strategy**.

You may choose **any indicators or models**, but every component must be justified.

```python
"""
Strategy: [Generated Name]
Author: Prashant Srivastava
Description: [One-line edge summary]
Timeframe: [Chosen timeframe]
Universe: [Pairs traded]
"""

import backtrader as bt
from dataclasses import dataclass
from typing import List
from enum import Enum
import logging

class SignalType(Enum):
    LONG = 1
    SHORT = -1
    FLAT = 0

@dataclass
class StrategyConfig:
    """
    Define ONLY parameters actually used by the strategy.
    """
    pairs: List[str]
    risk_per_trade: float
    max_positions: int
    max_portfolio_heat: float
    max_position_pct: float

    # Strategy-specific parameters
    ...
```

### STRATEGY CLASS IMPLEMENTATION

Your strategy MUST include the following methods:

- `__init__`
- `next`
- `notify_order`
- `notify_trade`
- `get_position_size`
- `check_entry_conditions`
- `check_exit_conditions`

Skeleton (logic must be YOUR OWN):

```python
class CoinDCXStrategy(bt.Strategy):
    """
    Entry Logic:
        [Explain clearly]

    Exit Logic:
        [Explain clearly]

    Risk Management:
        [Explain clearly]
    """

    params = dict(
        # Define parameters — no blind defaults
    )

    def __init__(self):
        pass

    def get_position_size(self) -> float:
        pass

    def check_entry_conditions(self) -> SignalType:
        pass

    def check_exit_conditions(self) -> bool:
        pass

    def next(self):
        pass
```

### COINDCX EXECUTION LAYER (STRUCTURAL ONLY)

Provide a minimal execution wrapper compatible with CoinDCX:

- HMAC authentication
- Balance retrieval
- Order placement
- Order status
- Order cancellation

**API Key Management:**

Use `python-dotenv` to securely load API credentials from a `.env` file:

```python
import os
from dotenv import load_dotenv

load_dotenv()

API_KEY = os.getenv("COINDCX_API_KEY")
API_SECRET = os.getenv("COINDCX_API_SECRET")
```

Example `.env` file (add to `.gitignore`):

```env
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
```

Execution must be:

- Fee-aware
- Exchange-safe
- Idempotent

Avoid unnecessary complexity.

---

### RISK MANAGEMENT (MANDATORY)

Implement a portfolio-level risk framework, including:

- Max position size
- Max portfolio heat
- Daily loss cutoff
- Drawdown-based de-risking
- Consecutive loss protection

Use a dataclass + manager pattern.

---

### FEE & TAX REALITY CHECK (MANDATORY)

You must:

- Adjust profitability for fees and slippage
- Adjust expected returns for Indian tax impact
- - Explicitly state the minimum edge required to remain profitable post-tax

---

### PERFORMANCE EXPECTATIONS (REALISTIC)

Provide defensible, non-marketing expectations:

- CAGR
- Max Drawdown
- Win Rate
- Profit Factor
- Sharpe Ratio
- Trades per month
- **Statistical Significance**: Ensure the strategy generates enough trades (N > 100) to be statistically significant. Consider lower timeframes (e.g., 3m, 5m, 15m, 4h) if necessary to achieve this sample size.

Explain why these metrics are realistic for this strategy.

---

### TRADE WALKTHROUGH (MANDATORY)

Provide:

- One complete winning trade
- One complete losing trade

Each must include:

- Entry logic
- Position size
- Stop & exit
- Fees
- Tax impact
- - Net P&L

---

### FINAL DELIVERABLES

- Original strategy hypothesis
- Complete Backtrader-compatible strategy code
- **Shared configuration system** (single source of truth for backtest & live)
- **Paper trading mode** for safe validation
- **Professional logging** (trade log + system log)
- Clearly defined entry & exit rules
- Risk & capital management framework
- Fee- and tax-adjusted edge validation
- Step-by-step roadmap to go live on CoinDCX
- **Walk-Forward Analysis Plan**: A plan to validate the strategy on out-of-sample data (e.g., bear market periods) to ensure robustness.

Do NOT optimize.
Do NOT curve-fit.
Do NOT overtrade.

Correctness, durability, and edge quality matter more than excitement.

