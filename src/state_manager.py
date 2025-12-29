"""
State Persistence Manager for Live Trading

Provides pluggable state persistence with SQLite primary storage
and JSON backup. Supports recovery from crashes, server restarts,
and graceful shutdown.

Features:
- Abstract interface for pluggable backends
- SQLite for durability and queryability
- JSON dump for human-readable backup
- Automatic state recovery on startup
- Position tracking with full audit trail
- Checkpoint system for cycle recovery
"""

import json
import logging
import os
import sqlite3
import threading
from abc import ABC, abstractmethod
from dataclasses import asdict, dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional

# =============================================================================
# LOGGING SETUP
# =============================================================================

logger = logging.getLogger(__name__)


class StateManagerLogger:
    """Dedicated logger for state management operations."""

    def __init__(self, name: str = "StateManager"):
        self.logger = logging.getLogger(f"state.{name}")
        self._setup_handlers()

    def _setup_handlers(self):
        """Setup console and file handlers if not already configured."""
        if not self.logger.handlers:
            self.logger.setLevel(logging.DEBUG)

            # Console handler
            console = logging.StreamHandler()
            console.setLevel(logging.INFO)
            console.setFormatter(
                logging.Formatter("%(asctime)s | %(levelname)-8s | %(name)s | %(message)s")
            )
            self.logger.addHandler(console)

    def info(self, msg: str, *args):
        self.logger.info(msg, *args)

    def debug(self, msg: str, *args):
        self.logger.debug(msg, *args)

    def warning(self, msg: str, *args):
        self.logger.warning(msg, *args)

    def error(self, msg: str, *args):
        self.logger.error(msg, *args)

    def critical(self, msg: str, *args):
        self.logger.critical(msg, *args)


# =============================================================================
# DATA MODELS
# =============================================================================


class PositionStatus(Enum):
    """Position lifecycle status."""

    PENDING = "pending"  # Order placed, not yet filled
    OPEN = "open"  # Position is active
    CLOSING = "closing"  # Exit order placed
    CLOSED = "closed"  # Position fully closed


class OrderSide(Enum):
    """Order side."""

    BUY = "buy"
    SELL = "sell"


@dataclass
class Position:
    """
    Represents a trading position with full state.

    Attributes:
        symbol: Trading pair (e.g., 'BTCINR')
        side: Position side (buy/sell)
        quantity: Position size
        entry_price: Average entry price
        entry_time: Position open timestamp
        stop_loss: Stop loss price level
        take_profit: Take profit price level
        status: Current position status
        order_id: Exchange order ID (if any)
        pnl: Realized P&L (after close)
        exit_price: Exit price (after close)
        exit_time: Position close timestamp
        metadata: Additional data (strategy signals, etc.)
    """

    symbol: str
    side: str = "buy"
    quantity: float = 0.0
    entry_price: float = 0.0
    entry_time: Optional[str] = None
    stop_loss: float = 0.0
    take_profit: float = 0.0
    status: str = "open"
    order_id: Optional[str] = None
    pnl: float = 0.0
    exit_price: float = 0.0
    exit_time: Optional[str] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization."""
        return asdict(self)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "Position":
        """Create Position from dictionary."""
        return cls(**data)

    def is_open(self) -> bool:
        """Check if position is still open."""
        return self.status in ("open", "pending", "closing")


@dataclass
class Checkpoint:
    """
    Checkpoint for cycle recovery.

    Stores the state at a point in time to allow recovery
    from crashes or restarts.
    """

    timestamp: str
    cycle_count: int
    portfolio_value: float
    cash: float
    positions_value: float
    open_positions: int
    last_processed_symbols: List[str]
    drawdown_pct: float = 0.0
    consecutive_losses: int = 0
    paper_mode: bool = True
    config_hash: str = ""
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization."""
        return asdict(self)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "Checkpoint":
        """Create Checkpoint from dictionary."""
        return cls(**data)


@dataclass
class TradeRecord:
    """
    Immutable trade record for audit trail.
    """

    id: Optional[int] = None
    symbol: str = ""
    side: str = "buy"
    quantity: float = 0.0
    entry_price: float = 0.0
    exit_price: float = 0.0
    entry_time: str = ""
    exit_time: str = ""
    pnl: float = 0.0
    fees: float = 0.0
    status: str = "closed"
    strategy_signal: str = ""
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary."""
        return asdict(self)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "TradeRecord":
        """Create from dictionary."""
        return cls(**data)


# =============================================================================
# ABSTRACT STATE MANAGER
# =============================================================================


class StateManager(ABC):
    """
    Abstract base class for state persistence.

    Implementations must provide methods for:
    - Position management (CRUD)
    - Checkpoint save/load
    - Trade history
    """

    @abstractmethod
    def initialize(self) -> bool:
        """Initialize the state manager. Returns True on success."""
        pass

    @abstractmethod
    def close(self) -> None:
        """Close connections and cleanup."""
        pass

    # Position Management
    @abstractmethod
    def save_position(self, position: Position) -> bool:
        """Save or update a position. Returns True on success."""
        pass

    @abstractmethod
    def load_positions(self, status: Optional[str] = None) -> List[Position]:
        """Load positions, optionally filtered by status."""
        pass

    @abstractmethod
    def get_position(self, symbol: str) -> Optional[Position]:
        """Get position for a specific symbol."""
        pass

    @abstractmethod
    def delete_position(self, symbol: str) -> bool:
        """Delete a position (use for cleanup)."""
        pass

    # Checkpoint Management
    @abstractmethod
    def save_checkpoint(self, checkpoint: Checkpoint) -> bool:
        """Save a checkpoint. Returns True on success."""
        pass

    @abstractmethod
    def load_checkpoint(self) -> Optional[Checkpoint]:
        """Load the most recent checkpoint."""
        pass

    # Trade History
    @abstractmethod
    def record_trade(self, trade: TradeRecord) -> bool:
        """Record a completed trade for audit trail."""
        pass

    @abstractmethod
    def get_trade_history(
        self, symbol: Optional[str] = None, limit: int = 100
    ) -> List[TradeRecord]:
        """Get trade history, optionally filtered by symbol."""
        pass

    # Utility
    @abstractmethod
    def export_json(self, filepath: str) -> bool:
        """Export current state to JSON file."""
        pass

    @abstractmethod
    def import_json(self, filepath: str) -> bool:
        """Import state from JSON file."""
        pass


# =============================================================================
# SQLITE STATE MANAGER
# =============================================================================


class SqliteStateManager(StateManager):
    """
    SQLite-based state persistence.

    Provides:
    - ACID transactions for data integrity
    - Full queryable history
    - Automatic schema migration
    - Thread-safe operations
    - JSON backup on each save
    """

    SCHEMA_VERSION = 1

    def __init__(
        self,
        db_path: str = "state/trading_state.db",
        json_backup_path: str = "state/trading_state.json",
        auto_backup: bool = True,
    ):
        """
        Initialize SQLite state manager.

        Args:
            db_path: Path to SQLite database file
            json_backup_path: Path for JSON backup
            auto_backup: Whether to auto-backup to JSON on changes
        """
        self.db_path = Path(db_path)
        self.json_backup_path = Path(json_backup_path)
        self.auto_backup = auto_backup
        self._conn: Optional[sqlite3.Connection] = None
        self._lock = threading.Lock()
        self.logger = StateManagerLogger("SQLite")

    def initialize(self) -> bool:
        """Initialize database and create tables."""
        try:
            # Ensure directory exists
            self.db_path.parent.mkdir(parents=True, exist_ok=True)
            self.json_backup_path.parent.mkdir(parents=True, exist_ok=True)

            # Connect to database
            self._conn = sqlite3.connect(
                str(self.db_path),
                check_same_thread=False,
                isolation_level="DEFERRED",
            )
            self._conn.row_factory = sqlite3.Row

            # Enable foreign keys and WAL mode for better concurrency
            self._conn.execute("PRAGMA foreign_keys = ON")
            self._conn.execute("PRAGMA journal_mode = WAL")

            # Create tables
            self._create_tables()

            self.logger.info("SQLite state manager initialized: %s", self.db_path)
            return True

        except Exception as e:
            self.logger.critical("Failed to initialize SQLite: %s", e)
            return False

    def _create_tables(self):
        """Create database schema."""
        with self._lock:
            cursor = self._conn.cursor()

            # Positions table
            cursor.execute(
                """
                CREATE TABLE IF NOT EXISTS positions (
                    symbol TEXT PRIMARY KEY,
                    side TEXT NOT NULL,
                    quantity REAL NOT NULL,
                    entry_price REAL NOT NULL,
                    entry_time TEXT,
                    stop_loss REAL,
                    take_profit REAL,
                    status TEXT NOT NULL DEFAULT 'open',
                    order_id TEXT,
                    pnl REAL DEFAULT 0,
                    exit_price REAL,
                    exit_time TEXT,
                    metadata TEXT DEFAULT '{}',
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                )
            """
            )

            # Checkpoints table
            cursor.execute(
                """
                CREATE TABLE IF NOT EXISTS checkpoints (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    cycle_count INTEGER NOT NULL,
                    portfolio_value REAL NOT NULL,
                    cash REAL NOT NULL,
                    positions_value REAL NOT NULL,
                    open_positions INTEGER NOT NULL,
                    last_processed_symbols TEXT NOT NULL,
                    drawdown_pct REAL DEFAULT 0,
                    consecutive_losses INTEGER DEFAULT 0,
                    paper_mode INTEGER DEFAULT 1,
                    config_hash TEXT,
                    metadata TEXT DEFAULT '{}',
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP
                )
            """
            )

            # Trade history table
            cursor.execute(
                """
                CREATE TABLE IF NOT EXISTS trades (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    symbol TEXT NOT NULL,
                    side TEXT NOT NULL,
                    quantity REAL NOT NULL,
                    entry_price REAL NOT NULL,
                    exit_price REAL,
                    entry_time TEXT NOT NULL,
                    exit_time TEXT,
                    pnl REAL DEFAULT 0,
                    fees REAL DEFAULT 0,
                    status TEXT DEFAULT 'open',
                    strategy_signal TEXT,
                    metadata TEXT DEFAULT '{}',
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP
                )
            """
            )

            # Create indexes
            cursor.execute(
                "CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status)"
            )
            cursor.execute("CREATE INDEX IF NOT EXISTS idx_trades_symbol ON trades(symbol)")
            cursor.execute(
                "CREATE INDEX IF NOT EXISTS idx_trades_entry_time ON trades(entry_time)"
            )
            cursor.execute(
                "CREATE INDEX IF NOT EXISTS idx_checkpoints_timestamp ON checkpoints(timestamp)"
            )

            # Schema version tracking
            cursor.execute(
                """
                CREATE TABLE IF NOT EXISTS schema_info (
                    key TEXT PRIMARY KEY,
                    value TEXT
                )
            """
            )
            cursor.execute(
                "INSERT OR REPLACE INTO schema_info (key, value) VALUES (?, ?)",
                ("version", str(self.SCHEMA_VERSION)),
            )

            self._conn.commit()
            self.logger.debug("Database schema created/verified")

    def close(self) -> None:
        """Close database connection."""
        if self._conn:
            # Final backup before closing
            if self.auto_backup:
                self.export_json(str(self.json_backup_path))
            self._conn.close()
            self._conn = None
            self.logger.info("SQLite connection closed")

    # -------------------------------------------------------------------------
    # Position Management
    # -------------------------------------------------------------------------

    def save_position(self, position: Position) -> bool:
        """Save or update a position."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                cursor.execute(
                    """
                    INSERT OR REPLACE INTO positions 
                    (symbol, side, quantity, entry_price, entry_time, stop_loss,
                     take_profit, status, order_id, pnl, exit_price, exit_time,
                     metadata, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                """,
                    (
                        position.symbol,
                        position.side,
                        position.quantity,
                        position.entry_price,
                        position.entry_time,
                        position.stop_loss,
                        position.take_profit,
                        position.status,
                        position.order_id,
                        position.pnl,
                        position.exit_price,
                        position.exit_time,
                        json.dumps(position.metadata),
                    ),
                )
                self._conn.commit()

            self.logger.debug(
                "Position saved: %s [%s] qty=%.6f @ %.2f",
                position.symbol,
                position.status,
                position.quantity,
                position.entry_price,
            )

            # Auto backup
            if self.auto_backup:
                self._backup_json()

            return True

        except Exception as e:
            self.logger.error("Failed to save position %s: %s", position.symbol, e)
            return False

    def load_positions(self, status: Optional[str] = None) -> List[Position]:
        """Load positions, optionally filtered by status."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                if status:
                    cursor.execute(
                        "SELECT * FROM positions WHERE status = ?", (status,)
                    )
                else:
                    cursor.execute("SELECT * FROM positions")

                rows = cursor.fetchall()

            positions = []
            for row in rows:
                positions.append(
                    Position(
                        symbol=row["symbol"],
                        side=row["side"],
                        quantity=row["quantity"],
                        entry_price=row["entry_price"],
                        entry_time=row["entry_time"],
                        stop_loss=row["stop_loss"] or 0.0,
                        take_profit=row["take_profit"] or 0.0,
                        status=row["status"],
                        order_id=row["order_id"],
                        pnl=row["pnl"] or 0.0,
                        exit_price=row["exit_price"] or 0.0,
                        exit_time=row["exit_time"],
                        metadata=json.loads(row["metadata"] or "{}"),
                    )
                )

            self.logger.debug(
                "Loaded %d positions (filter: %s)", len(positions), status or "all"
            )
            return positions

        except Exception as e:
            self.logger.error("Failed to load positions: %s", e)
            return []

    def get_position(self, symbol: str) -> Optional[Position]:
        """Get position for a specific symbol."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                cursor.execute("SELECT * FROM positions WHERE symbol = ?", (symbol,))
                row = cursor.fetchone()

            if not row:
                return None

            return Position(
                symbol=row["symbol"],
                side=row["side"],
                quantity=row["quantity"],
                entry_price=row["entry_price"],
                entry_time=row["entry_time"],
                stop_loss=row["stop_loss"] or 0.0,
                take_profit=row["take_profit"] or 0.0,
                status=row["status"],
                order_id=row["order_id"],
                pnl=row["pnl"] or 0.0,
                exit_price=row["exit_price"] or 0.0,
                exit_time=row["exit_time"],
                metadata=json.loads(row["metadata"] or "{}"),
            )

        except Exception as e:
            self.logger.error("Failed to get position %s: %s", symbol, e)
            return None

    def delete_position(self, symbol: str) -> bool:
        """Delete a position."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                cursor.execute("DELETE FROM positions WHERE symbol = ?", (symbol,))
                self._conn.commit()
                deleted = cursor.rowcount > 0

            if deleted:
                self.logger.info("Position deleted: %s", symbol)
                if self.auto_backup:
                    self._backup_json()
            return deleted

        except Exception as e:
            self.logger.error("Failed to delete position %s: %s", symbol, e)
            return False

    # -------------------------------------------------------------------------
    # Checkpoint Management
    # -------------------------------------------------------------------------

    def save_checkpoint(self, checkpoint: Checkpoint) -> bool:
        """Save a checkpoint."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                cursor.execute(
                    """
                    INSERT INTO checkpoints 
                    (timestamp, cycle_count, portfolio_value, cash, positions_value,
                     open_positions, last_processed_symbols, drawdown_pct,
                     consecutive_losses, paper_mode, config_hash, metadata)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                    (
                        checkpoint.timestamp,
                        checkpoint.cycle_count,
                        checkpoint.portfolio_value,
                        checkpoint.cash,
                        checkpoint.positions_value,
                        checkpoint.open_positions,
                        json.dumps(checkpoint.last_processed_symbols),
                        checkpoint.drawdown_pct,
                        checkpoint.consecutive_losses,
                        1 if checkpoint.paper_mode else 0,
                        checkpoint.config_hash,
                        json.dumps(checkpoint.metadata),
                    ),
                )
                self._conn.commit()

            self.logger.debug(
                "Checkpoint saved: cycle=%d, value=%.2f, positions=%d",
                checkpoint.cycle_count,
                checkpoint.portfolio_value,
                checkpoint.open_positions,
            )

            if self.auto_backup:
                self._backup_json()

            return True

        except Exception as e:
            self.logger.error("Failed to save checkpoint: %s", e)
            return False

    def load_checkpoint(self) -> Optional[Checkpoint]:
        """Load the most recent checkpoint."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                cursor.execute(
                    "SELECT * FROM checkpoints ORDER BY id DESC LIMIT 1"
                )
                row = cursor.fetchone()

            if not row:
                self.logger.debug("No checkpoint found")
                return None

            checkpoint = Checkpoint(
                timestamp=row["timestamp"],
                cycle_count=row["cycle_count"],
                portfolio_value=row["portfolio_value"],
                cash=row["cash"],
                positions_value=row["positions_value"],
                open_positions=row["open_positions"],
                last_processed_symbols=json.loads(row["last_processed_symbols"]),
                drawdown_pct=row["drawdown_pct"] or 0.0,
                consecutive_losses=row["consecutive_losses"] or 0,
                paper_mode=bool(row["paper_mode"]),
                config_hash=row["config_hash"] or "",
                metadata=json.loads(row["metadata"] or "{}"),
            )

            self.logger.info(
                "Loaded checkpoint: cycle=%d, value=%.2f, time=%s",
                checkpoint.cycle_count,
                checkpoint.portfolio_value,
                checkpoint.timestamp,
            )

            return checkpoint

        except Exception as e:
            self.logger.error("Failed to load checkpoint: %s", e)
            return None

    # -------------------------------------------------------------------------
    # Trade History
    # -------------------------------------------------------------------------

    def record_trade(self, trade: TradeRecord) -> bool:
        """Record a completed trade."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                cursor.execute(
                    """
                    INSERT INTO trades 
                    (symbol, side, quantity, entry_price, exit_price, entry_time,
                     exit_time, pnl, fees, status, strategy_signal, metadata)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                    (
                        trade.symbol,
                        trade.side,
                        trade.quantity,
                        trade.entry_price,
                        trade.exit_price,
                        trade.entry_time,
                        trade.exit_time,
                        trade.pnl,
                        trade.fees,
                        trade.status,
                        trade.strategy_signal,
                        json.dumps(trade.metadata),
                    ),
                )
                self._conn.commit()

            self.logger.info(
                "Trade recorded: %s %s %.6f @ %.2f -> %.2f, P&L=%.2f",
                trade.side.upper(),
                trade.symbol,
                trade.quantity,
                trade.entry_price,
                trade.exit_price,
                trade.pnl,
            )

            return True

        except Exception as e:
            self.logger.error("Failed to record trade: %s", e)
            return False

    def get_trade_history(
        self, symbol: Optional[str] = None, limit: int = 100
    ) -> List[TradeRecord]:
        """Get trade history."""
        try:
            with self._lock:
                cursor = self._conn.cursor()
                if symbol:
                    cursor.execute(
                        "SELECT * FROM trades WHERE symbol = ? ORDER BY id DESC LIMIT ?",
                        (symbol, limit),
                    )
                else:
                    cursor.execute(
                        "SELECT * FROM trades ORDER BY id DESC LIMIT ?", (limit,)
                    )
                rows = cursor.fetchall()

            trades = []
            for row in rows:
                trades.append(
                    TradeRecord(
                        id=row["id"],
                        symbol=row["symbol"],
                        side=row["side"],
                        quantity=row["quantity"],
                        entry_price=row["entry_price"],
                        exit_price=row["exit_price"] or 0.0,
                        entry_time=row["entry_time"],
                        exit_time=row["exit_time"] or "",
                        pnl=row["pnl"] or 0.0,
                        fees=row["fees"] or 0.0,
                        status=row["status"],
                        strategy_signal=row["strategy_signal"] or "",
                        metadata=json.loads(row["metadata"] or "{}"),
                    )
                )

            return trades

        except Exception as e:
            self.logger.error("Failed to get trade history: %s", e)
            return []

    # -------------------------------------------------------------------------
    # JSON Import/Export
    # -------------------------------------------------------------------------

    def _backup_json(self):
        """Internal method to backup to JSON."""
        try:
            self.export_json(str(self.json_backup_path))
        except Exception as e:
            self.logger.warning("JSON backup failed: %s", e)

    def export_json(self, filepath: str) -> bool:
        """Export current state to JSON file."""
        try:
            positions = self.load_positions()
            checkpoint = self.load_checkpoint()
            trades = self.get_trade_history(limit=1000)

            state = {
                "exported_at": datetime.now().isoformat(),
                "schema_version": self.SCHEMA_VERSION,
                "positions": [p.to_dict() for p in positions],
                "checkpoint": checkpoint.to_dict() if checkpoint else None,
                "recent_trades": [t.to_dict() for t in trades],
            }

            filepath = Path(filepath)
            filepath.parent.mkdir(parents=True, exist_ok=True)

            with open(filepath, "w") as f:
                json.dump(state, f, indent=2, default=str)

            self.logger.debug("State exported to: %s", filepath)
            return True

        except Exception as e:
            self.logger.error("Failed to export JSON: %s", e)
            return False

    def import_json(self, filepath: str) -> bool:
        """Import state from JSON file."""
        try:
            filepath = Path(filepath)
            if not filepath.exists():
                self.logger.warning("Import file not found: %s", filepath)
                return False

            with open(filepath) as f:
                state = json.load(f)

            # Import positions
            for pos_data in state.get("positions", []):
                position = Position.from_dict(pos_data)
                self.save_position(position)

            self.logger.info(
                "Imported state from JSON: %d positions",
                len(state.get("positions", [])),
            )
            return True

        except Exception as e:
            self.logger.error("Failed to import JSON: %s", e)
            return False


# =============================================================================
# JSON FILE STATE MANAGER (FALLBACK)
# =============================================================================


class JsonFileStateManager(StateManager):
    """
    Simple JSON file-based state manager.

    Use as fallback when SQLite is not available or for simple deployments.
    """

    def __init__(self, filepath: str = "state/trading_state.json"):
        self.filepath = Path(filepath)
        self._state: Dict[str, Any] = {
            "positions": {},
            "checkpoint": None,
            "trades": [],
        }
        self._lock = threading.Lock()
        self.logger = StateManagerLogger("JsonFile")

    def initialize(self) -> bool:
        """Initialize and load existing state."""
        try:
            self.filepath.parent.mkdir(parents=True, exist_ok=True)

            if self.filepath.exists():
                with open(self.filepath) as f:
                    self._state = json.load(f)
                self.logger.info("Loaded existing state from: %s", self.filepath)
            else:
                self._save()
                self.logger.info("Created new state file: %s", self.filepath)

            return True

        except Exception as e:
            self.logger.error("Failed to initialize: %s", e)
            return False

    def close(self) -> None:
        """Save state before closing."""
        self._save()
        self.logger.info("State saved and closed")

    def _save(self):
        """Save state to file."""
        with self._lock:
            with open(self.filepath, "w") as f:
                json.dump(self._state, f, indent=2, default=str)

    def save_position(self, position: Position) -> bool:
        """Save a position."""
        try:
            with self._lock:
                self._state["positions"][position.symbol] = position.to_dict()
            self._save()
            self.logger.debug("Position saved: %s", position.symbol)
            return True
        except Exception as e:
            self.logger.error("Failed to save position: %s", e)
            return False

    def load_positions(self, status: Optional[str] = None) -> List[Position]:
        """Load positions."""
        positions = []
        for data in self._state.get("positions", {}).values():
            pos = Position.from_dict(data)
            if status is None or pos.status == status:
                positions.append(pos)
        return positions

    def get_position(self, symbol: str) -> Optional[Position]:
        """Get a specific position."""
        data = self._state.get("positions", {}).get(symbol)
        return Position.from_dict(data) if data else None

    def delete_position(self, symbol: str) -> bool:
        """Delete a position."""
        try:
            with self._lock:
                if symbol in self._state.get("positions", {}):
                    del self._state["positions"][symbol]
                    self._save()
                    return True
            return False
        except Exception as e:
            self.logger.error("Failed to delete position: %s", e)
            return False

    def save_checkpoint(self, checkpoint: Checkpoint) -> bool:
        """Save checkpoint."""
        try:
            with self._lock:
                self._state["checkpoint"] = checkpoint.to_dict()
            self._save()
            return True
        except Exception as e:
            self.logger.error("Failed to save checkpoint: %s", e)
            return False

    def load_checkpoint(self) -> Optional[Checkpoint]:
        """Load checkpoint."""
        data = self._state.get("checkpoint")
        return Checkpoint.from_dict(data) if data else None

    def record_trade(self, trade: TradeRecord) -> bool:
        """Record a trade."""
        try:
            with self._lock:
                if "trades" not in self._state:
                    self._state["trades"] = []
                self._state["trades"].append(trade.to_dict())
                # Keep last 1000 trades
                self._state["trades"] = self._state["trades"][-1000:]
            self._save()
            return True
        except Exception as e:
            self.logger.error("Failed to record trade: %s", e)
            return False

    def get_trade_history(
        self, symbol: Optional[str] = None, limit: int = 100
    ) -> List[TradeRecord]:
        """Get trade history."""
        trades = self._state.get("trades", [])
        if symbol:
            trades = [t for t in trades if t.get("symbol") == symbol]
        trades = trades[-limit:]
        return [TradeRecord.from_dict(t) for t in trades]

    def export_json(self, filepath: str) -> bool:
        """Export to JSON (just copy)."""
        try:
            import shutil
            shutil.copy(self.filepath, filepath)
            return True
        except Exception as e:
            self.logger.error("Failed to export: %s", e)
            return False

    def import_json(self, filepath: str) -> bool:
        """Import from JSON."""
        try:
            with open(filepath) as f:
                self._state = json.load(f)
            self._save()
            return True
        except Exception as e:
            self.logger.error("Failed to import: %s", e)
            return False


# =============================================================================
# FACTORY FUNCTION
# =============================================================================


def create_state_manager(
    backend: str = "sqlite",
    state_dir: str = "state",
    **kwargs,
) -> StateManager:
    """
    Factory function to create appropriate state manager.

    Args:
        backend: 'sqlite' or 'json'
        state_dir: Directory for state files
        **kwargs: Additional arguments for the specific backend

    Returns:
        Initialized StateManager instance
    """
    logger = StateManagerLogger("Factory")

    if backend == "sqlite":
        db_path = kwargs.get("db_path", f"{state_dir}/trading_state.db")
        json_backup = kwargs.get("json_backup_path", f"{state_dir}/trading_state.json")
        auto_backup = kwargs.get("auto_backup", True)

        manager = SqliteStateManager(
            db_path=db_path,
            json_backup_path=json_backup,
            auto_backup=auto_backup,
        )

    elif backend == "json":
        filepath = kwargs.get("filepath", f"{state_dir}/trading_state.json")
        manager = JsonFileStateManager(filepath=filepath)

    else:
        logger.warning("Unknown backend '%s', falling back to JSON", backend)
        manager = JsonFileStateManager(filepath=f"{state_dir}/trading_state.json")

    # Initialize
    if not manager.initialize():
        logger.error("Failed to initialize %s state manager", backend)
        # Fallback to JSON if SQLite fails
        if backend == "sqlite":
            logger.warning("Falling back to JSON state manager")
            manager = JsonFileStateManager(filepath=f"{state_dir}/trading_state.json")
            manager.initialize()

    return manager


# =============================================================================
# CONVENIENCE FUNCTIONS
# =============================================================================


def get_open_positions(manager: StateManager) -> List[Position]:
    """Get all open positions."""
    return manager.load_positions(status="open")


def has_position(manager: StateManager, symbol: str) -> bool:
    """Check if there's an open position for symbol."""
    pos = manager.get_position(symbol)
    return pos is not None and pos.is_open()


def close_position(
    manager: StateManager,
    symbol: str,
    exit_price: float,
    pnl: float,
) -> bool:
    """Close a position and record the trade."""
    pos = manager.get_position(symbol)
    if not pos:
        return False

    # Update position
    pos.status = "closed"
    pos.exit_price = exit_price
    pos.exit_time = datetime.now().isoformat()
    pos.pnl = pnl
    manager.save_position(pos)

    # Record trade
    trade = TradeRecord(
        symbol=pos.symbol,
        side=pos.side,
        quantity=pos.quantity,
        entry_price=pos.entry_price,
        exit_price=exit_price,
        entry_time=pos.entry_time or "",
        exit_time=pos.exit_time or "",
        pnl=pnl,
        status="closed",
    )
    manager.record_trade(trade)

    return True
