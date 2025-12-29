"""
CoinDCX Execution Layer

A minimal, production-grade execution wrapper for CoinDCX API.
Handles authentication, balance retrieval, order management, and execution.

Features:
- HMAC SHA256 authentication
- Rate limiting
- Retry logic with exponential backoff
- Order idempotency
- Fee-aware execution
"""

import os
import time
import hmac
import hashlib
import json
import logging
from typing import Optional, Dict, Any, List
from dataclasses import dataclass
from enum import Enum
from datetime import datetime

import requests
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

# Configure logging
logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s")
logger = logging.getLogger(__name__)


class OrderSide(Enum):
    """Order side enumeration"""

    BUY = "buy"
    SELL = "sell"


class OrderType(Enum):
    """Order type enumeration"""

    LIMIT = "limit_order"
    MARKET = "market_order"


class OrderStatus(Enum):
    """Order status enumeration"""

    OPEN = "open"
    PARTIALLY_FILLED = "partially_filled"
    FILLED = "filled"
    CANCELLED = "cancelled"
    REJECTED = "rejected"


@dataclass
class OrderRequest:
    """Order request parameters"""

    market: str  # e.g., "BTCINR"
    side: OrderSide
    order_type: OrderType
    quantity: float
    price: Optional[float] = None  # Required for limit orders
    client_order_id: Optional[str] = None  # For idempotency


@dataclass
class OrderResponse:
    """Order response data"""

    order_id: str
    client_order_id: Optional[str]
    market: str
    side: str
    status: str
    price: float
    quantity: float
    filled_quantity: float
    timestamp: datetime
    fee: float = 0.0


@dataclass
class Balance:
    """Account balance data"""

    currency: str
    balance: float
    locked_balance: float
    available_balance: float


class CoinDCXExecutionError(Exception):
    """Custom exception for CoinDCX execution errors"""


class CoinDCXClient:
    """
    CoinDCX API Client

    Provides authenticated access to CoinDCX trading API.
    Implements best practices for exchange communication:
    - Proper HMAC authentication
    - Rate limiting
    - Retry with backoff
    - Error handling
    """

    BASE_URL = "https://api.coindcx.com"

    # Rate limits (requests per second)
    RATE_LIMIT = 10

    # Fee structure
    MAKER_FEE = 0.001  # 0.1%
    TAKER_FEE = 0.001  # 0.1%

    def __init__(
        self,
        api_key: Optional[str] = None,
        api_secret: Optional[str] = None,
        _testnet: bool = False,  # Not available for CoinDCX, kept for API compatibility
    ):
        """
        Initialize CoinDCX client.

        Args:
            api_key: CoinDCX API key (defaults to env var)
            api_secret: CoinDCX API secret (defaults to env var)
            _testnet: Unused (testnet not available for CoinDCX)
        """
        self.api_key = api_key or os.getenv("COINDCX_API_KEY")
        self.api_secret = api_secret or os.getenv("COINDCX_API_SECRET")

        if not self.api_key or not self.api_secret:
            logger.warning("API credentials not provided. Only public endpoints available.")

        self._last_request_time = 0
        self._request_count = 0

    def _generate_signature(self, body: Dict[str, Any]) -> str:
        """
        Generate HMAC SHA256 signature for request body.

        Args:
            body: Request body dictionary

        Returns:
            Hex-encoded signature string
        """
        json_body = json.dumps(body, separators=(",", ":"))
        signature = hmac.new(
            self.api_secret.encode("utf-8"), json_body.encode("utf-8"), hashlib.sha256
        ).hexdigest()
        return signature

    def _rate_limit(self):
        """Implement rate limiting"""
        current_time = time.time()
        time_diff = current_time - self._last_request_time

        if time_diff < 1.0 / self.RATE_LIMIT:
            sleep_time = (1.0 / self.RATE_LIMIT) - time_diff
            time.sleep(sleep_time)

        self._last_request_time = time.time()

    def _make_request(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        authenticated: bool = True,
        retries: int = 3,
    ) -> Dict[str, Any]:
        """
        Make HTTP request to CoinDCX API.

        Args:
            method: HTTP method (GET, POST, DELETE)
            endpoint: API endpoint
            body: Request body
            authenticated: Whether request requires authentication
            retries: Number of retries on failure

        Returns:
            Response data dictionary

        Raises:
            CoinDCXExecutionError: On API error
        """
        url = f"{self.BASE_URL}{endpoint}"
        headers = {"Content-Type": "application/json"}

        if body is None:
            body = {}

        # Add timestamp for authenticated requests
        if authenticated:
            if not self.api_key or not self.api_secret:
                raise CoinDCXExecutionError("API credentials required for authenticated endpoints")

            body["timestamp"] = int(time.time() * 1000)
            signature = self._generate_signature(body)
            headers["X-AUTH-APIKEY"] = self.api_key
            headers["X-AUTH-SIGNATURE"] = signature

        # Rate limit
        self._rate_limit()

        # Retry logic
        last_error = None
        for attempt in range(retries):
            try:
                if method == "GET":
                    response = requests.get(url, headers=headers, timeout=30)
                elif method == "POST":
                    response = requests.post(url, json=body, headers=headers, timeout=30)
                elif method == "DELETE":
                    response = requests.delete(url, json=body, headers=headers, timeout=30)
                else:
                    raise CoinDCXExecutionError(f"Unsupported HTTP method: {method}")

                # Check response status
                if response.status_code == 200:
                    return response.json()

                if response.status_code == 429:
                    # Rate limited - exponential backoff
                    wait_time = (2**attempt) * 1
                    logger.warning("Rate limited. Waiting %ds before retry...", wait_time)
                    time.sleep(wait_time)
                    continue

                error_msg = response.text
                try:
                    error_data = response.json()
                    error_msg = error_data.get("message", error_msg)
                except ValueError:
                    pass
                raise CoinDCXExecutionError(f"API error {response.status_code}: {error_msg}")

            except requests.exceptions.Timeout:
                last_error = "Request timeout"
                logger.warning("Request timeout. Attempt %d/%d", attempt + 1, retries)
                time.sleep(2**attempt)
            except requests.exceptions.ConnectionError as e:
                last_error = str(e)
                logger.warning("Connection error: %s. Attempt %d/%d", e, attempt + 1, retries)
                time.sleep(2**attempt)

        raise CoinDCXExecutionError(f"Request failed after {retries} attempts: {last_error}")

    # =========================================================================
    # PUBLIC ENDPOINTS
    # =========================================================================

    def get_markets(self) -> List[Dict[str, Any]]:
        """
        Get all available trading markets.

        Returns:
            List of market dictionaries
        """
        return self._make_request("GET", "/exchange/v1/markets_details", authenticated=False)

    def get_ticker(self, market: str) -> Dict[str, Any]:
        """
        Get ticker data for a market.

        Args:
            market: Market symbol (e.g., "BTCINR")

        Returns:
            Ticker data dictionary
        """
        tickers = self._make_request("GET", "/exchange/ticker", authenticated=False)
        for ticker in tickers:
            if ticker.get("market") == market:
                return ticker
        raise CoinDCXExecutionError(f"Market not found: {market}")

    def get_orderbook(self, market: str) -> Dict[str, Any]:
        """
        Get order book for a market.

        Args:
            market: Market symbol

        Returns:
            Order book dictionary with bids and asks
        """
        body = {"pair": market}
        return self._make_request("POST", "/market_data/orderbook", body=body, authenticated=False)

    # =========================================================================
    # AUTHENTICATED ENDPOINTS - ACCOUNT
    # =========================================================================

    def get_balances(self) -> List[Balance]:
        """
        Get account balances.

        Returns:
            List of Balance objects
        """
        response = self._make_request("POST", "/exchange/v1/users/balances")

        balances = []
        for item in response:
            balance = Balance(
                currency=item["currency"],
                balance=float(item.get("balance", 0)),
                locked_balance=float(item.get("locked_balance", 0)),
                available_balance=float(item.get("balance", 0))
                - float(item.get("locked_balance", 0)),
            )
            balances.append(balance)

        return balances

    def get_balance(self, currency: str) -> Balance:
        """
        Get balance for specific currency.

        Args:
            currency: Currency code (e.g., "INR", "BTC")

        Returns:
            Balance object
        """
        balances = self.get_balances()
        for b in balances:
            if b.currency.upper() == currency.upper():
                return b
        raise CoinDCXExecutionError(f"Currency not found: {currency}")

    # =========================================================================
    # AUTHENTICATED ENDPOINTS - ORDERS
    # =========================================================================

    def place_order(self, order: OrderRequest) -> OrderResponse:
        """
        Place a new order.

        Args:
            order: OrderRequest object

        Returns:
            OrderResponse object
        """
        body = {
            "market": order.market,
            "side": order.side.value,
            "order_type": order.order_type.value,
            "total_quantity": order.quantity,
        }

        if order.order_type == OrderType.LIMIT:
            if order.price is None:
                raise CoinDCXExecutionError("Price required for limit orders")
            body["price_per_unit"] = order.price

        if order.client_order_id:
            body["client_order_id"] = order.client_order_id

        # Select appropriate endpoint
        endpoint = "/exchange/v1/orders/create"

        response = self._make_request("POST", endpoint, body=body)

        return OrderResponse(
            order_id=response.get("id", ""),
            client_order_id=response.get("client_order_id"),
            market=response.get("market", order.market),
            side=response.get("side", order.side.value),
            status=response.get("status", "open"),
            price=float(response.get("price_per_unit", order.price or 0)),
            quantity=float(response.get("total_quantity", order.quantity)),
            filled_quantity=float(response.get("filled_quantity", 0)),
            timestamp=datetime.now(),
            fee=float(response.get("fee", 0)),
        )

    def place_market_buy(self, market: str, quantity: float) -> OrderResponse:
        """
        Place a market buy order.

        Args:
            market: Market symbol
            quantity: Quantity to buy

        Returns:
            OrderResponse object
        """
        order = OrderRequest(
            market=market,
            side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            quantity=quantity,
            client_order_id=f"MKT_BUY_{int(time.time() * 1000)}",
        )
        return self.place_order(order)

    def place_market_sell(self, market: str, quantity: float) -> OrderResponse:
        """
        Place a market sell order.

        Args:
            market: Market symbol
            quantity: Quantity to sell

        Returns:
            OrderResponse object
        """
        order = OrderRequest(
            market=market,
            side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            quantity=quantity,
            client_order_id=f"MKT_SELL_{int(time.time() * 1000)}",
        )
        return self.place_order(order)

    def place_limit_buy(self, market: str, quantity: float, price: float) -> OrderResponse:
        """
        Place a limit buy order.

        Args:
            market: Market symbol
            quantity: Quantity to buy
            price: Limit price

        Returns:
            OrderResponse object
        """
        order = OrderRequest(
            market=market,
            side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=quantity,
            price=price,
            client_order_id=f"LMT_BUY_{int(time.time() * 1000)}",
        )
        return self.place_order(order)

    def place_limit_sell(self, market: str, quantity: float, price: float) -> OrderResponse:
        """
        Place a limit sell order.

        Args:
            market: Market symbol
            quantity: Quantity to sell
            price: Limit price

        Returns:
            OrderResponse object
        """
        order = OrderRequest(
            market=market,
            side=OrderSide.SELL,
            order_type=OrderType.LIMIT,
            quantity=quantity,
            price=price,
            client_order_id=f"LMT_SELL_{int(time.time() * 1000)}",
        )
        return self.place_order(order)

    def get_order_status(self, order_id: str) -> OrderResponse:
        """
        Get status of an order.

        Args:
            order_id: Order ID

        Returns:
            OrderResponse object
        """
        body = {"id": order_id}
        response = self._make_request("POST", "/exchange/v1/orders/status", body=body)

        return OrderResponse(
            order_id=response.get("id", order_id),
            client_order_id=response.get("client_order_id"),
            market=response.get("market", ""),
            side=response.get("side", ""),
            status=response.get("status", ""),
            price=float(response.get("price_per_unit", 0)),
            quantity=float(response.get("total_quantity", 0)),
            filled_quantity=float(response.get("filled_quantity", 0)),
            timestamp=datetime.now(),
            fee=float(response.get("fee", 0)),
        )

    def cancel_order(self, order_id: str) -> bool:
        """
        Cancel an open order.

        Args:
            order_id: Order ID to cancel

        Returns:
            True if cancelled successfully
        """
        body = {"id": order_id}
        try:
            self._make_request("POST", "/exchange/v1/orders/cancel", body=body)
            logger.info("Order %s cancelled successfully", order_id)
            return True
        except CoinDCXExecutionError as e:
            logger.error("Failed to cancel order %s: %s", order_id, e)
            return False

    def cancel_all_orders(self, market: Optional[str] = None) -> int:
        """
        Cancel all open orders.

        Args:
            market: Optional market to filter (cancels all if None)

        Returns:
            Number of orders cancelled
        """
        body = {}
        if market:
            body["market"] = market

        try:
            response = self._make_request("POST", "/exchange/v1/orders/cancel_all", body=body)
            cancelled = len(response) if isinstance(response, list) else 0
            logger.info("Cancelled %d orders", cancelled)
            return cancelled
        except CoinDCXExecutionError as e:
            logger.error("Failed to cancel orders: %s", e)
            return 0

    def get_open_orders(self, market: Optional[str] = None) -> List[OrderResponse]:
        """
        Get all open orders.

        Args:
            market: Optional market to filter

        Returns:
            List of OrderResponse objects
        """
        body = {}
        if market:
            body["market"] = market

        response = self._make_request("POST", "/exchange/v1/orders/active_orders", body=body)

        orders = []
        for item in response.get("orders", []):
            order = OrderResponse(
                order_id=item.get("id", ""),
                client_order_id=item.get("client_order_id"),
                market=item.get("market", ""),
                side=item.get("side", ""),
                status=item.get("status", "open"),
                price=float(item.get("price_per_unit", 0)),
                quantity=float(item.get("total_quantity", 0)),
                filled_quantity=float(item.get("filled_quantity", 0)),
                timestamp=datetime.now(),
                fee=float(item.get("fee", 0)),
            )
            orders.append(order)

        return orders

    def get_trade_history(
        self, market: Optional[str] = None, limit: int = 100
    ) -> List[Dict[str, Any]]:
        """
        Get trade history.

        Args:
            market: Optional market to filter
            limit: Maximum number of trades to return

        Returns:
            List of trade dictionaries
        """
        body = {"limit": limit}
        if market:
            body["market"] = market

        return self._make_request("POST", "/exchange/v1/orders/trade_history", body=body)

    # =========================================================================
    # UTILITY METHODS
    # =========================================================================

    def calculate_fees(self, quantity: float, price: float, is_maker: bool = False) -> float:
        """
        Calculate trading fees.

        Args:
            quantity: Order quantity
            price: Order price
            is_maker: Whether order is a maker order

        Returns:
            Fee amount in quote currency
        """
        value = quantity * price
        fee_rate = self.MAKER_FEE if is_maker else self.TAKER_FEE
        return value * fee_rate

    def get_minimum_order_size(self, market: str) -> float:
        """
        Get minimum order size for a market.

        Args:
            market: Market symbol

        Returns:
            Minimum order size
        """
        markets = self.get_markets()
        for m in markets:
            if m.get("symbol") == market:
                return float(m.get("min_quantity", 0))
        return 0.0

    def get_candles(
        self,
        symbol: str,
        timeframe: str = "1d",
        limit: int = 100,
    ) -> List[Dict[str, Any]]:
        """
        Fetch historical OHLCV candle data from CoinDCX public API.

        Args:
            symbol: Trading symbol (e.g., 'BTCINR')
            timeframe: Candle interval (1m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 1d, 3d, 1w, 1M)
            limit: Number of candles to fetch (max 1000)

        Returns:
            List of candle dictionaries with keys: time, open, high, low, close, volume
        """
        # Convert symbol format: BTCINR -> I-BTC_INR
        if symbol.endswith("INR") and not symbol.startswith("I-"):
            base = symbol[:-3]  # Remove INR suffix
            pair = f"I-{base}_INR"
        else:
            pair = symbol

        # Map common timeframe formats
        tf_map = {
            "1D": "1d", "1d": "1d",
            "4H": "4h", "4h": "4h",
            "1H": "1h", "1h": "1h",
            "15M": "15m", "15m": "15m",
        }
        interval = tf_map.get(timeframe, timeframe)

        url = "https://public.coindcx.com/market_data/candles"
        params = {
            "pair": pair,
            "interval": interval,
            "limit": min(limit, 1000),
        }

        try:
            self._rate_limit()
            response = requests.get(url, params=params, timeout=30)
            response.raise_for_status()
            data = response.json()

            if not data:
                logger.warning("No candle data returned for %s", symbol)
                return []

            return data

        except Exception as e:
            logger.error("Failed to fetch candles for %s: %s", symbol, e)
            raise CoinDCXExecutionError(f"Failed to fetch candles: {e}")


class CoinDCXExecutor:
    """
    High-level execution manager for strategy integration.

    Provides:
    - Fee-aware order execution
    - Position management
    - Order tracking
    - Execution logging
    """

    def __init__(self, client: Optional[CoinDCXClient] = None):
        """
        Initialize executor.

        Args:
            client: CoinDCX client instance
        """
        self.client = client or CoinDCXClient()
        self.open_orders: Dict[str, OrderResponse] = {}
        self.positions: Dict[str, float] = {}  # market -> quantity

    def execute_buy(
        self,
        market: str,
        quantity: float,
        limit_price: Optional[float] = None,
        use_limit: bool = True,
    ) -> Optional[OrderResponse]:
        """
        Execute a buy order with fee awareness.

        Args:
            market: Market symbol
            quantity: Quantity to buy
            limit_price: Limit price (if use_limit=True)
            use_limit: Use limit order (recommended)

        Returns:
            OrderResponse or None on failure
        """
        try:
            # Check minimum order size
            min_size = self.client.get_minimum_order_size(market)
            if quantity < min_size:
                logger.warning("Order size %s below minimum %s", quantity, min_size)
                return None

            if use_limit and limit_price:
                response = self.client.place_limit_buy(market, quantity, limit_price)
            else:
                response = self.client.place_market_buy(market, quantity)

            self.open_orders[response.order_id] = response
            logger.info(
                "BUY order placed: %s - %s @ %s",
                response.order_id,
                quantity,
                limit_price or "MARKET",
            )

            return response

        except CoinDCXExecutionError as e:
            logger.error("Buy execution failed: %s", e)
            return None

    def execute_sell(
        self,
        market: str,
        quantity: float,
        limit_price: Optional[float] = None,
        use_limit: bool = True,
    ) -> Optional[OrderResponse]:
        """
        Execute a sell order with fee awareness.

        Args:
            market: Market symbol
            quantity: Quantity to sell
            limit_price: Limit price (if use_limit=True)
            use_limit: Use limit order (recommended)

        Returns:
            OrderResponse or None on failure
        """
        try:
            # Check available balance
            base_currency = market.replace("INR", "")
            balance = self.client.get_balance(base_currency)

            if balance.available_balance < quantity:
                logger.warning("Insufficient balance: %s < %s", balance.available_balance, quantity)
                quantity = balance.available_balance

            if quantity <= 0:
                return None

            if use_limit and limit_price:
                response = self.client.place_limit_sell(market, quantity, limit_price)
            else:
                response = self.client.place_market_sell(market, quantity)

            self.open_orders[response.order_id] = response
            logger.info(
                "SELL order placed: %s - %s @ %s",
                response.order_id,
                quantity,
                limit_price or "MARKET",
            )

            return response

        except CoinDCXExecutionError as e:
            logger.error("Sell execution failed: %s", e)
            return None

    def sync_positions(self) -> Dict[str, float]:
        """
        Synchronize positions from exchange balances.

        Returns:
            Dictionary of market -> position size
        """
        try:
            balances = self.client.get_balances()
            self.positions = {}

            for balance in balances:
                if balance.balance > 0 and balance.currency != "INR":
                    market = f"{balance.currency}INR"
                    self.positions[market] = balance.available_balance

            return self.positions

        except CoinDCXExecutionError as e:
            logger.error("Failed to sync positions: %s", e)
            return {}

    def cancel_all_open_orders(self, market: Optional[str] = None) -> int:
        """
        Cancel all open orders.

        Args:
            market: Optional market filter

        Returns:
            Number of orders cancelled
        """
        cancelled = self.client.cancel_all_orders(market)
        self.open_orders.clear()
        return cancelled
