from typing import Type, Dict, Any, List
import backtrader as bt
from .volatility_regime import CoinDCXStrategy
from .volatility_regime.config import get_default_params as get_volatility_regime_defaults
from .volatility_regime.config import get_optimization_grid as get_volatility_regime_grid
from .volatility_regime.config import get_aliases as get_volatility_regime_aliases
from .bollinger_reversion import BollingerReversionStrategy
from .bollinger_reversion.config import get_default_params as get_bollinger_reversion_defaults
from .bollinger_reversion.config import get_optimization_grid as get_bollinger_reversion_grid
from .bollinger_reversion.config import get_aliases as get_bollinger_reversion_aliases

STRATEGY_REGISTRY = {
    "volatility_regime": CoinDCXStrategy,
    "bollinger_reversion": BollingerReversionStrategy,
    "default": CoinDCXStrategy,
}

DEFAULTS_REGISTRY = {
    "volatility_regime": get_volatility_regime_defaults,
    "bollinger_reversion": get_bollinger_reversion_defaults,
    "default": get_volatility_regime_defaults,
}

OPTIMIZATION_REGISTRY = {
    "volatility_regime": get_volatility_regime_grid,
    "bollinger_reversion": get_bollinger_reversion_grid,
    "default": get_volatility_regime_grid,
}

ALIAS_REGISTRY = {
    "volatility_regime": get_volatility_regime_aliases,
    "bollinger_reversion": get_bollinger_reversion_aliases,
    "default": get_volatility_regime_aliases,
}


def get_strategy_class(strategy_name: str) -> Type[bt.Strategy]:
    return STRATEGY_REGISTRY.get(strategy_name, STRATEGY_REGISTRY["default"])


def get_strategy_defaults(strategy_name: str) -> Dict[str, Any]:
    factory = DEFAULTS_REGISTRY.get(strategy_name, DEFAULTS_REGISTRY["default"])
    return factory()


def get_strategy_optimization_grid(strategy_name: str, mode: str = "quick") -> Dict[str, List[Any]]:
    factory = OPTIMIZATION_REGISTRY.get(strategy_name, OPTIMIZATION_REGISTRY["default"])
    return factory(mode=mode)


def get_strategy_aliases(strategy_name: str) -> Dict[str, str]:
    factory = ALIAS_REGISTRY.get(strategy_name, ALIAS_REGISTRY["default"])
    return factory()
