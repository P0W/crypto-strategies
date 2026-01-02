"""
Configuration Module

Centralized configuration management for the trading strategy.
"""

from dataclasses import dataclass, field
from typing import List, Optional, Dict, Any
from pathlib import Path
import os
import json
import logging

from dotenv import load_dotenv
from .strategies import get_strategy_defaults

# Load environment variables
load_dotenv()

logger = logging.getLogger(__name__)


@dataclass
class ExchangeConfig:
    """CoinDCX exchange configuration"""

    api_key: str = field(default_factory=lambda: os.getenv("COINDCX_API_KEY", ""))
    api_secret: str = field(default_factory=lambda: os.getenv("COINDCX_API_SECRET", ""))
    maker_fee: float = 0.001  # 0.1%
    taker_fee: float = 0.001  # 0.1%
    assumed_slippage: float = 0.001  # 0.1%
    rate_limit: int = 10  # requests per second


@dataclass
class TradingConfig:
    """Trading parameters configuration"""

    # Universe - SOL is best performer from optimization
    pairs: List[str] = field(
        default_factory=lambda: ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"]
    )
    timeframe: str = "1d"  # Best timeframe from optimization

    # Capital
    initial_capital: float = 100_000.0  # ₹1 Lakh

    # Risk per trade
    risk_per_trade: float = 0.15  # 15%
    max_positions: int = 5
    max_portfolio_heat: float = 0.30  # 30%
    max_position_pct: float = 0.20  # 20%

    # Drawdown limits
    max_drawdown: float = 0.20  # 20%
    drawdown_warning: float = 0.10  # 10% - reduce position size by 50%
    drawdown_critical: float = 0.15  # 15% - reduce position size by 75%

    # Drawdown position size multipliers
    drawdown_warning_multiplier: float = 0.50  # 50% of normal size at warning level
    drawdown_critical_multiplier: float = 0.25  # 25% of normal size at critical level

    # Consecutive loss protection
    consecutive_loss_limit: int = 3  # Number of losses before reducing size
    consecutive_loss_multiplier: float = 0.75  # 75% of normal size after losses


@dataclass
class StrategyConfig:
    """Strategy-specific parameters"""

    # Strategy Selection
    name: str = "default"

    # Generic parameters dictionary
    params: Dict[str, Any] = field(default_factory=dict)


@dataclass
class TaxConfig:
    """Indian tax configuration"""

    tax_rate: float = 0.30  # 30% flat tax on gains
    tds_rate: float = 0.01  # 1% TDS
    loss_offset_allowed: bool = False  # No loss offset in India


@dataclass
class BacktestConfig:
    """Backtest configuration"""

    data_dir: str = "data"
    results_dir: str = "results"
    timeframe: str = "4h"  # Timeframe for data files (1h, 4h, 1d)
    start_date: Optional[str] = None  # YYYY-MM-DD format
    end_date: Optional[str] = None
    commission: float = 0.001  # 0.1%
    slippage: float = 0.001


@dataclass
class Config:
    """Master configuration container"""

    exchange: ExchangeConfig = field(default_factory=ExchangeConfig)
    trading: TradingConfig = field(default_factory=TradingConfig)
    strategy: StrategyConfig = field(default_factory=StrategyConfig)
    tax: TaxConfig = field(default_factory=TaxConfig)
    backtest: BacktestConfig = field(default_factory=BacktestConfig)

    def __post_init__(self):
        """Initialize strategy params with defaults if empty"""
        if not self.strategy.params:
            self.strategy.params = get_strategy_defaults(self.strategy.name)

    @classmethod
    def load_from_file(cls, path: str) -> "Config":
        """Load configuration from JSON file (supports flat or nested format)"""
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)

        config = cls()

        # Check if nested format (has 'trading', 'strategy' keys)
        if "trading" in data or "strategy" in data:
            if "exchange" in data:
                config.exchange = ExchangeConfig(**data["exchange"])
            if "trading" in data:
                config.trading = TradingConfig(**data["trading"])
            if "strategy" in data:
                # Handle strategy config
                strat_data = data["strategy"]
                # Backward compatibility: check for old 'strategy_name' at root level
                strategy_name = strat_data.get("name")
                if not strategy_name and "strategy_name" in data:
                    strategy_name = data["strategy_name"]
                config.strategy.name = strategy_name or "default"

                # Load defaults for this strategy
                defaults = get_strategy_defaults(config.strategy.name)

                # Merge defaults with provided params
                # If 'params' key exists, use it, otherwise treat remaining keys as params
                if "params" in strat_data:
                    config.strategy.params = {**defaults, **strat_data["params"]}
                else:
                    # Filter out 'name' and treat rest as params
                    provided_params = {k: v for k, v in strat_data.items() if k != "name"}
                    config.strategy.params = {**defaults, **provided_params}

            if "tax" in data:
                config.tax = TaxConfig(**data["tax"])
            if "backtest" in data:
                config.backtest = BacktestConfig(**data["backtest"])
        else:
            # Flat format - map to appropriate sections
            # Trading config
            if "pairs" in data:
                config.trading.pairs = data["pairs"]
            if "timeframe" in data:
                config.trading.timeframe = data["timeframe"]
                config.backtest.timeframe = data["timeframe"]
            if "initial_capital" in data:
                config.trading.initial_capital = data["initial_capital"]
            if "position_size_pct" in data:
                config.trading.risk_per_trade = data["position_size_pct"]
            if "risk_per_trade" in data:
                config.trading.risk_per_trade = data["risk_per_trade"]
            if "max_positions" in data:
                config.trading.max_positions = data["max_positions"]
            if "max_portfolio_heat" in data:
                config.trading.max_portfolio_heat = data["max_portfolio_heat"]
            if "max_position_pct" in data:
                config.trading.max_position_pct = data["max_position_pct"]
            if "max_drawdown" in data:
                config.trading.max_drawdown = data["max_drawdown"]

            # Drawdown protection
            if "drawdown_warning" in data:
                config.trading.drawdown_warning = data["drawdown_warning"]
            if "drawdown_critical" in data:
                config.trading.drawdown_critical = data["drawdown_critical"]
            if "drawdown_warning_multiplier" in data:
                config.trading.drawdown_warning_multiplier = data["drawdown_warning_multiplier"]
            if "drawdown_critical_multiplier" in data:
                config.trading.drawdown_critical_multiplier = data["drawdown_critical_multiplier"]

            # Consecutive loss protection
            if "consecutive_loss_limit" in data:
                config.trading.consecutive_loss_limit = data["consecutive_loss_limit"]
            if "consecutive_loss_multiplier" in data:
                config.trading.consecutive_loss_multiplier = data["consecutive_loss_multiplier"]

            # Strategy config - Flat format assumes default strategy
            config.strategy.name = "default"
            defaults = get_strategy_defaults("default")

            # Extract strategy params from flat dict
            strategy_params = {}
            for key in defaults.keys():
                if key in data:
                    strategy_params[key] = data[key]

            config.strategy.params = {**defaults, **strategy_params}

            # Exchange config
            if "maker_fee" in data:
                config.exchange.maker_fee = data["maker_fee"]
            if "taker_fee" in data:
                config.exchange.taker_fee = data["taker_fee"]
            if "slippage" in data:
                config.exchange.assumed_slippage = data["slippage"]

        return config

    def save_to_file(self, path: str):
        """Save configuration to JSON file"""
        data = {
            "exchange": {
                "maker_fee": self.exchange.maker_fee,
                "taker_fee": self.exchange.taker_fee,
                "assumed_slippage": self.exchange.assumed_slippage,
                "rate_limit": self.exchange.rate_limit,
            },
            "trading": {
                "pairs": self.trading.pairs,
                "timeframe": self.trading.timeframe,
                "initial_capital": self.trading.initial_capital,
                "risk_per_trade": self.trading.risk_per_trade,
                "max_positions": self.trading.max_positions,
                "max_portfolio_heat": self.trading.max_portfolio_heat,
                "max_position_pct": self.trading.max_position_pct,
                "max_drawdown": self.trading.max_drawdown,
                # Drawdown protection
                "drawdown_warning": self.trading.drawdown_warning,
                "drawdown_critical": self.trading.drawdown_critical,
                "drawdown_warning_multiplier": self.trading.drawdown_warning_multiplier,
                "drawdown_critical_multiplier": self.trading.drawdown_critical_multiplier,
                # Consecutive loss protection
                "consecutive_loss_limit": self.trading.consecutive_loss_limit,
                "consecutive_loss_multiplier": self.trading.consecutive_loss_multiplier,
            },
            "strategy": {"name": self.strategy.name, **self.strategy.params},
            "tax": {
                "tax_rate": self.tax.tax_rate,
                "tds_rate": self.tax.tds_rate,
                "loss_offset_allowed": self.tax.loss_offset_allowed,
            },
            "backtest": {
                "data_dir": self.backtest.data_dir,
                "results_dir": self.backtest.results_dir,
                "start_date": self.backtest.start_date,
                "end_date": self.backtest.end_date,
                "commission": self.backtest.commission,
            },
        }

        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2)

        logger.info("Configuration saved to %s", path)

    def get_strategy_params(self, **extra_params) -> dict:
        """
        Get strategy parameters dictionary for Backtrader strategy.

        This is the SINGLE SOURCE OF TRUTH for strategy parameters.
        Used by both backtest.py and live_trader.py to ensure identical config.

        Args:
            **extra_params: Additional params (e.g., logger for live trading)

        Returns:
            Dictionary of parameters for CoinDCXStrategy
        """
        params = {
            # Risk parameters (from trading config)
            "risk_per_trade": self.trading.risk_per_trade,
            "max_positions": self.trading.max_positions,
            "max_portfolio_heat": self.trading.max_portfolio_heat,
            "max_position_pct": self.trading.max_position_pct,
            "max_drawdown": self.trading.max_drawdown,
            # Drawdown protection (from trading config)
            "drawdown_warning": self.trading.drawdown_warning,
            "drawdown_critical": self.trading.drawdown_critical,
            "drawdown_warning_multiplier": self.trading.drawdown_warning_multiplier,
            "drawdown_critical_multiplier": self.trading.drawdown_critical_multiplier,
            # Consecutive loss protection (from trading config)
            "consecutive_loss_limit": self.trading.consecutive_loss_limit,
            "consecutive_loss_multiplier": self.trading.consecutive_loss_multiplier,
            # Fees (from exchange config)
            "maker_fee": self.exchange.maker_fee,
            "taker_fee": self.exchange.taker_fee,
            "slippage": self.exchange.assumed_slippage,
        }

        # Merge strategy specific params
        params.update(self.strategy.params)

        # Merge with any extra params (e.g., logger, executor for live trading)
        params.update(extra_params)

        return params


def get_default_config() -> Config:
    """Get default configuration"""
    return Config()


def setup_logging(level: int = logging.INFO, log_file: Optional[str] = None):
    """
    Setup logging configuration.

    Args:
        level: Logging level
        log_file: Optional file path for logging
    """
    handlers: List[logging.Handler] = [logging.StreamHandler()]

    if log_file:
        Path(log_file).parent.mkdir(parents=True, exist_ok=True)
        handlers.append(logging.FileHandler(log_file))

    logging.basicConfig(
        level=level,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
        handlers=handlers,
    )

    # Reduce noise from external libraries
    logging.getLogger("urllib3").setLevel(logging.WARNING)
    logging.getLogger("requests").setLevel(logging.WARNING)
