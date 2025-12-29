from typing import Dict, Any, List

def get_default_params() -> Dict[str, Any]:
    """
    Default configuration for Volatility Regime Strategy.
    """
    return {
        # Volatility regime
        "atr_period": 14,
        "volatility_lookback": 20,
        "compression_threshold": 0.6,
        "expansion_threshold": 1.5,
        "extreme_threshold": 2.5,

        # Trend confirmation
        "ema_fast": 8,
        "ema_slow": 21,
        "adx_period": 14,
        "adx_threshold": 30.0,

        # Entry/Exit
        "breakout_atr_multiple": 1.5,
        "stop_atr_multiple": 2.5,
        "target_atr_multiple": 5.0,
        "trailing_activation": 0.5,
        "trailing_atr_multiple": 1.5,
    }

def get_optimization_grid(mode: str = "quick") -> Dict[str, List[Any]]:
    """
    Returns the parameter grid for optimization.
    """
    if mode == "quick":
        return {
            "param_grid": {
                "adx_threshold": [20.0, 25.0, 30.0],
                "stop_atr_multiple": [2.5, 3.0, 3.5],
                "target_atr_multiple": [5.0, 6.0, 7.0],
            },
            "timeframes": ["4h"],
            "coins": ["BTC", "ETH", "SOL", "XRP", "DOGE"]
        }
    
    if mode == "custom":
        return {
            "param_grid": {},
            "timeframes": ["1d"],
            "coins": ["BTC"]
        }

def get_aliases() -> Dict[str, str]:
    """
    CLI parameter aliases.
    """
    return {
        "adx": "adx_threshold",
        "stop_atr": "stop_atr_multiple",
        "target_atr": "target_atr_multiple",
        "breakout_atr": "breakout_atr_multiple",
        "sl": "stop_atr_multiple",
        "tp": "target_atr_multiple",
        "atr": "atr_period",
        "ema_fast": "ema_fast",
        "ema_slow": "ema_slow",
    }
    
    # Full mode
    return {
        "param_grid": {
            "adx_threshold": [20.0, 25.0, 30.0],
            "stop_atr_multiple": [2.0, 2.5, 3.0, 3.5],
            "target_atr_multiple": [4.0, 5.0, 6.0, 7.0],
            "compression_threshold": [0.6, 0.7],
        },
        "timeframes": ["1h", "4h", "1d"],
        "coins": ["BTC", "ETH", "SOL", "XRP", "DOGE", "MATIC", "ADA", "AVAX"]
    }
