from typing import Dict, Any, List

def get_default_params() -> Dict[str, Any]:
    """
    Returns default parameters for the Bollinger Reversion strategy.
    These will be merged into the generic StrategyConfig.params dictionary.
    """
    return {
        # Strategy Specific
        "bb_period": 20,
        "bb_dev": 1.5,  # Tighter bands for more signals in 5m
        "rsi_period": 14,
        "rsi_oversold": 45,  # Relaxed RSI for more signals
        "rsi_overbought": 55, # Relaxed RSI for more signals
        
        # Risk Defaults for this strategy (Scalping needs tighter stops)
        "stop_loss_pct": 0.015,  # 1.5% stop loss
        "risk_per_trade": 0.02,  # 2% risk per trade
    }

def get_optimization_grid(mode: str = "quick") -> Dict[str, List[Any]]:
    """
    Returns the parameter grid for optimization.
    """
    if mode == "quick":
        return {
            "param_grid": {
                "bb_period": [20],
                "bb_dev": [2.0],
                "rsi_period": [14],
                "rsi_oversold": [30, 40],
                "rsi_overbought": [60, 70],
            },
            "timeframes": ["15m"],
            "coins": ["BTC", "ETH", "SOL"]
        }

    # Full mode
    return {
        "param_grid": {
            "bb_period": [15, 20, 25],
            "bb_dev": [1.5, 2.0, 2.5],
            "rsi_period": [10, 14, 20],
            "rsi_oversold": [25, 30, 35, 40],
            "rsi_overbought": [60, 65, 70, 75],
        },
        "timeframes": ["1h", "4h", "1d"],
        "coins": ["BTC", "ETH", "SOL", "XRP", "DOGE", "MATIC"]
    }

def get_aliases() -> Dict[str, str]:
    """
    CLI parameter aliases.
    """
    return {
        "bb": "bb_period",
        "dev": "bb_dev",
        "rsi": "rsi_period",
        "oversold": "rsi_oversold",
        "overbought": "rsi_overbought",
        "sl_pct": "stop_loss_pct",
    }
