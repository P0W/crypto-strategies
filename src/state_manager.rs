// State Manager for Live Trading
// SQLite-based persistence with JSON backup
//
// Provides position tracking, checkpoints, and trade audit trail
// matching the Python implementation for production deployment.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

// =============================================================================
// Data Models
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub side: String, // "buy" or "sell"
    pub quantity: f64,
    pub entry_price: f64,
    pub entry_time: Option<String>,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub status: String, // "pending", "open", "closing", "closed"
    pub order_id: Option<String>,
    pub pnl: f64,
    pub exit_price: f64,
    pub exit_time: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Position {
    pub fn is_open(&self) -> bool {
        matches!(self.status.as_str(), "open" | "pending" | "closing")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub timestamp: String,
    pub cycle_count: i32,
    pub portfolio_value: f64,
    pub cash: f64,
    pub positions_value: f64,
    pub open_positions: i32,
    pub last_processed_symbols: Vec<String>,
    pub drawdown_pct: f64,
    pub consecutive_losses: i32,
    pub paper_mode: bool,
    pub config_hash: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: Option<i64>,
    pub symbol: String,
    pub side: String,
    pub quantity: f64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub entry_time: String,
    pub exit_time: String,
    // P&L breakdown
    pub gross_pnl: f64,
    pub fees: f64,
    pub tax: f64,
    pub net_pnl: f64,
    pub pnl_pct: f64,
    // Strategy context
    pub status: String,
    pub exit_reason: String,
    pub strategy_signal: String,
    pub market_state_entry: String,
    pub market_state_exit: String,
    // Risk management
    pub atr_at_entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub risk_reward_actual: f64,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOrder {
    pub order_id: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub quantity: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub client_id: Option<String>,
}

// =============================================================================
// State Manager Implementation
// =============================================================================

pub struct SqliteStateManager {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
    json_backup_path: PathBuf,
    auto_backup: bool,
}

impl SqliteStateManager {
    pub fn new<P: AsRef<Path>>(db_path: P, json_backup_path: P, auto_backup: bool) -> Result<Self> {
        let db_path_ref = db_path.as_ref();

        // Create parent directories
        if let Some(parent) = db_path_ref.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = json_backup_path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path_ref)
            .with_context(|| format!("Failed to open database: {}", db_path_ref.display()))?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let manager = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: db_path_ref.to_path_buf(),
            json_backup_path: json_backup_path.as_ref().to_path_buf(),
            auto_backup,
        };

        manager.create_tables()?;
        info!("SQLite state manager initialized");

        Ok(manager)
    }

    fn create_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS positions (
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
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS checkpoints (
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
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS trades (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                quantity REAL NOT NULL,
                entry_price REAL NOT NULL,
                exit_price REAL,
                entry_time TEXT NOT NULL,
                exit_time TEXT,
                gross_pnl REAL DEFAULT 0,
                fees REAL DEFAULT 0,
                tax REAL DEFAULT 0,
                net_pnl REAL DEFAULT 0,
                pnl_pct REAL DEFAULT 0,
                status TEXT DEFAULT 'open',
                exit_reason TEXT,
                strategy_signal TEXT,
                market_state_entry TEXT,
                market_state_exit TEXT,
                atr_at_entry REAL DEFAULT 0,
                stop_loss REAL DEFAULT 0,
                take_profit REAL DEFAULT 0,
                risk_reward_actual REAL DEFAULT 0,
                metadata TEXT DEFAULT '{}',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        // Create indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trades_symbol ON trades(symbol)",
            [],
        )?;

        // Orders table for pending order persistence
        conn.execute(
            "CREATE TABLE IF NOT EXISTS pending_orders (
                order_id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                side TEXT NOT NULL,
                order_type TEXT NOT NULL,
                quantity REAL NOT NULL,
                limit_price REAL,
                stop_price REAL,
                client_id TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_orders_symbol ON pending_orders(symbol)",
            [],
        )?;

        debug!("Database schema created/verified");
        Ok(())
    }

    pub fn save_position(&self, pos: &Position) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let metadata_json = serde_json::to_string(&pos.metadata)?;

        conn.execute(
            "INSERT OR REPLACE INTO positions 
             (symbol, side, quantity, entry_price, entry_time, stop_loss,
              take_profit, status, order_id, pnl, exit_price, exit_time,
              metadata, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, CURRENT_TIMESTAMP)",
            params![
                pos.symbol,
                pos.side,
                pos.quantity,
                pos.entry_price,
                pos.entry_time,
                pos.stop_loss,
                pos.take_profit,
                pos.status,
                pos.order_id,
                pos.pnl,
                pos.exit_price,
                pos.exit_time,
                metadata_json,
            ],
        )?;

        debug!(
            "Position saved: {} [{}] qty={:.6} @ {:.2}",
            pos.symbol, pos.status, pos.quantity, pos.entry_price
        );

        if self.auto_backup {
            drop(conn);
            self.export_json()?;
        }

        Ok(())
    }

    pub fn load_positions(&self, status_filter: Option<&str>) -> Result<Vec<Position>> {
        let conn = self.conn.lock().unwrap();

        let query = if let Some(status) = status_filter {
            format!("SELECT * FROM positions WHERE status = '{}'", status)
        } else {
            "SELECT * FROM positions".to_string()
        };

        let mut stmt = conn.prepare(&query)?;
        let positions = stmt
            .query_map([], |row| {
                Ok(Position {
                    symbol: row.get(0)?,
                    side: row.get(1)?,
                    quantity: row.get(2)?,
                    entry_price: row.get(3)?,
                    entry_time: row.get(4)?,
                    stop_loss: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                    take_profit: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                    status: row.get(7)?,
                    order_id: row.get(8)?,
                    pnl: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                    exit_price: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                    exit_time: row.get(11)?,
                    metadata: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        debug!(
            "Loaded {} positions (filter: {:?})",
            positions.len(),
            status_filter
        );
        Ok(positions)
    }

    pub fn get_position(&self, symbol: &str) -> Result<Option<Position>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM positions WHERE symbol = ?1")?;

        let pos = stmt.query_row(params![symbol], |row| {
            Ok(Position {
                symbol: row.get(0)?,
                side: row.get(1)?,
                quantity: row.get(2)?,
                entry_price: row.get(3)?,
                entry_time: row.get(4)?,
                stop_loss: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                take_profit: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                status: row.get(7)?,
                order_id: row.get(8)?,
                pnl: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                exit_price: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                exit_time: row.get(11)?,
                metadata: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
            })
        });

        match pos {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_checkpoint(&self, ckpt: &Checkpoint) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let symbols_json = serde_json::to_string(&ckpt.last_processed_symbols)?;
        let metadata_json = serde_json::to_string(&ckpt.metadata)?;

        conn.execute(
            "INSERT INTO checkpoints 
             (timestamp, cycle_count, portfolio_value, cash, positions_value,
              open_positions, last_processed_symbols, drawdown_pct,
              consecutive_losses, paper_mode, config_hash, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                ckpt.timestamp,
                ckpt.cycle_count,
                ckpt.portfolio_value,
                ckpt.cash,
                ckpt.positions_value,
                ckpt.open_positions,
                symbols_json,
                ckpt.drawdown_pct,
                ckpt.consecutive_losses,
                if ckpt.paper_mode { 1 } else { 0 },
                ckpt.config_hash,
                metadata_json,
            ],
        )?;

        debug!(
            "Checkpoint saved: cycle={}, value={:.2}",
            ckpt.cycle_count, ckpt.portfolio_value
        );

        if self.auto_backup {
            drop(conn);
            self.export_json()?;
        }

        Ok(())
    }

    pub fn load_checkpoint(&self) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM checkpoints ORDER BY id DESC LIMIT 1")?;

        let ckpt = stmt.query_row([], |row| {
            Ok(Checkpoint {
                timestamp: row.get(1)?,
                cycle_count: row.get(2)?,
                portfolio_value: row.get(3)?,
                cash: row.get(4)?,
                positions_value: row.get(5)?,
                open_positions: row.get(6)?,
                last_processed_symbols: serde_json::from_str(&row.get::<_, String>(7)?)
                    .unwrap_or_default(),
                drawdown_pct: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                consecutive_losses: row.get::<_, Option<i32>>(9)?.unwrap_or(0),
                paper_mode: row.get::<_, i32>(10)? != 0,
                config_hash: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                metadata: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
            })
        });

        match ckpt {
            Ok(c) => {
                info!(
                    "Loaded checkpoint: cycle={}, value={:.2}",
                    c.cycle_count, c.portfolio_value
                );
                Ok(Some(c))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                debug!("No checkpoint found");
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn record_trade(&self, trade: &TradeRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let metadata_json = serde_json::to_string(&trade.metadata)?;

        conn.execute(
            "INSERT INTO trades 
             (symbol, side, quantity, entry_price, exit_price, entry_time,
              exit_time, gross_pnl, fees, tax, net_pnl, pnl_pct, status,
              exit_reason, strategy_signal, market_state_entry, market_state_exit,
              atr_at_entry, stop_loss, take_profit, risk_reward_actual, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                trade.symbol,
                trade.side,
                trade.quantity,
                trade.entry_price,
                trade.exit_price,
                trade.entry_time,
                trade.exit_time,
                trade.gross_pnl,
                trade.fees,
                trade.tax,
                trade.net_pnl,
                trade.pnl_pct,
                trade.status,
                trade.exit_reason,
                trade.strategy_signal,
                trade.market_state_entry,
                trade.market_state_exit,
                trade.atr_at_entry,
                trade.stop_loss,
                trade.take_profit,
                trade.risk_reward_actual,
                metadata_json,
            ],
        )?;

        let result = if trade.net_pnl > 0.0 { "WIN" } else { "LOSS" };
        info!(
            "Trade recorded: {} {} {:.6} @ Rs {:.2} -> Rs {:.2} | Net: Rs {:.2} ({:+.2}%) | {} | {}",
            trade.side.to_uppercase(),
            trade.symbol,
            trade.quantity,
            trade.entry_price,
            trade.exit_price,
            trade.net_pnl,
            trade.pnl_pct,
            trade.exit_reason,
            result
        );

        Ok(())
    }

    /// Save a pending order to the database
    pub fn save_pending_order(&self, order: &PendingOrder) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO pending_orders 
             (order_id, symbol, side, order_type, quantity, limit_price, stop_price, client_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                order.order_id,
                order.symbol,
                order.side,
                order.order_type,
                order.quantity,
                order.limit_price,
                order.stop_price,
                order.client_id,
            ],
        )?;
        debug!("Pending order saved: {} {}", order.side, order.symbol);
        Ok(())
    }

    /// Load all pending orders from the database
    pub fn load_pending_orders(&self) -> Result<Vec<PendingOrder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM pending_orders")?;

        let orders = stmt
            .query_map([], |row| {
                Ok(PendingOrder {
                    order_id: row.get(0)?,
                    symbol: row.get(1)?,
                    side: row.get(2)?,
                    order_type: row.get(3)?,
                    quantity: row.get(4)?,
                    limit_price: row.get(5)?,
                    stop_price: row.get(6)?,
                    client_id: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(orders)
    }

    /// Clear all pending orders (called after orders are restored to orderbook)
    pub fn clear_pending_orders(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pending_orders", [])?;
        debug!("Pending orders cleared");
        Ok(())
    }

    /// Remove a specific pending order by ID
    pub fn remove_pending_order(&self, order_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM pending_orders WHERE order_id = ?1",
            params![order_id],
        )?;
        Ok(())
    }

    pub fn export_json(&self) -> Result<()> {
        let positions = self.load_positions(None)?;
        let checkpoint = self.load_checkpoint()?;
        let pending_orders = self.load_pending_orders()?;

        let state = serde_json::json!({
            "exported_at": Utc::now().to_rfc3339(),
            "positions": positions,
            "checkpoint": checkpoint,
            "pending_orders": pending_orders,
        });

        std::fs::write(
            &self.json_backup_path,
            serde_json::to_string_pretty(&state)?,
        )?;
        debug!("State exported to: {}", self.json_backup_path.display());
        Ok(())
    }

    // Async wrappers for use in async contexts (like live trading)
    pub async fn save_checkpoint_async(
        &self,
        portfolio_value: f64,
        position_count: u32,
    ) -> Result<()> {
        let checkpoint = Checkpoint {
            timestamp: Utc::now().to_rfc3339(),
            cycle_count: 0, // Would increment in production
            portfolio_value,
            cash: portfolio_value, // Simplified - in production, calculate actual cash
            positions_value: 0.0,
            open_positions: position_count as i32,
            last_processed_symbols: vec![],
            drawdown_pct: 0.0,
            consecutive_losses: 0,
            paper_mode: true,
            config_hash: String::new(),
            metadata: HashMap::new(),
        };
        let state_manager = self.clone_for_async();
        tokio::task::spawn_blocking(move || state_manager.save_checkpoint(&checkpoint)).await?
    }

    pub async fn save_trade_async(&self, trade: &crate::Trade) -> Result<()> {
        // Convert from simple Trade to full TradeRecord
        let trade_record = TradeRecord {
            id: None, // Auto-assigned by database
            symbol: trade.symbol.as_str().to_string(),
            side: format!("{:?}", trade.side).to_lowercase(),
            quantity: trade.quantity.to_f64(),
            entry_price: trade.entry_price.to_f64(),
            exit_price: trade.exit_price.to_f64(),
            entry_time: trade.entry_time.to_rfc3339(),
            exit_time: trade.exit_time.to_rfc3339(),
            gross_pnl: trade.pnl.to_f64(),
            fees: trade.commission.to_f64(),
            tax: if trade.net_pnl.is_positive() {
                trade.net_pnl.to_f64() * 0.3
            } else {
                0.0
            },
            net_pnl: trade.net_pnl.to_f64(),
            pnl_pct: trade.return_pct(),
            status: "closed".to_string(),
            exit_reason: "signal".to_string(),
            strategy_signal: "flat".to_string(),
            market_state_entry: "unknown".to_string(),
            market_state_exit: "unknown".to_string(),
            atr_at_entry: 0.0,
            stop_loss: 0.0,
            take_profit: 0.0,
            risk_reward_actual: 0.0,
            metadata: HashMap::new(),
        };

        let state_manager = self.clone_for_async();
        tokio::task::spawn_blocking(move || state_manager.record_trade(&trade_record)).await?
    }

    fn clone_for_async(&self) -> Self {
        // Create a new state manager with the same paths
        SqliteStateManager::new(
            self.db_path.clone(),
            self.json_backup_path.clone(),
            false, // Don't auto-export for clones
        )
        .expect("Failed to clone state manager")
    }
}

// =============================================================================
// Factory Function
// =============================================================================

pub fn create_state_manager<P: AsRef<Path>>(
    state_dir: P,
    _backend: &str,
) -> Result<SqliteStateManager> {
    let state_dir = state_dir.as_ref();
    std::fs::create_dir_all(state_dir)?;

    let db_path = state_dir.join("trading_state.db");
    let json_path = state_dir.join("trading_state.json");

    SqliteStateManager::new(db_path, json_path, true)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper to create a test state manager with a temporary directory
    fn create_test_manager() -> (SqliteStateManager, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.db");
        let json_path = temp_dir.path().join("test.json");
        let manager =
            SqliteStateManager::new(&db_path, &json_path, false).expect("Failed to create manager");
        (manager, temp_dir)
    }

    /// Helper to create a test position
    fn create_test_position(symbol: &str) -> Position {
        Position {
            symbol: symbol.to_string(),
            side: "buy".to_string(),
            quantity: 1.5,
            entry_price: 50000.0,
            entry_time: Some("2024-01-15T10:30:00Z".to_string()),
            stop_loss: 48000.0,
            take_profit: 55000.0,
            status: "open".to_string(),
            order_id: Some("order_123".to_string()),
            pnl: 0.0,
            exit_price: 0.0,
            exit_time: None,
            metadata: HashMap::new(),
        }
    }

    /// Helper to create a test checkpoint
    fn create_test_checkpoint(cycle: i32) -> Checkpoint {
        Checkpoint {
            timestamp: Utc::now().to_rfc3339(),
            cycle_count: cycle,
            portfolio_value: 10500.0,
            cash: 5000.0,
            positions_value: 5500.0,
            open_positions: 2,
            last_processed_symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
            drawdown_pct: 5.0,
            consecutive_losses: 1,
            paper_mode: true,
            config_hash: "abc123".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Helper to create a test trade record
    fn create_test_trade(symbol: &str) -> TradeRecord {
        TradeRecord {
            id: None,
            symbol: symbol.to_string(),
            side: "buy".to_string(),
            quantity: 0.5,
            entry_price: 50000.0,
            exit_price: 52000.0,
            entry_time: "2024-01-15T10:00:00Z".to_string(),
            exit_time: "2024-01-15T14:00:00Z".to_string(),
            gross_pnl: 1000.0,
            fees: 10.0,
            tax: 297.0,
            net_pnl: 693.0,
            pnl_pct: 4.0,
            status: "closed".to_string(),
            exit_reason: "target".to_string(),
            strategy_signal: "flat".to_string(),
            market_state_entry: "normal".to_string(),
            market_state_exit: "normal".to_string(),
            atr_at_entry: 1500.0,
            stop_loss: 48000.0,
            take_profit: 55000.0,
            risk_reward_actual: 2.5,
            metadata: HashMap::new(),
        }
    }

    /// Helper to create a test pending order
    fn create_test_pending_order(order_id: &str, symbol: &str) -> PendingOrder {
        PendingOrder {
            order_id: order_id.to_string(),
            symbol: symbol.to_string(),
            side: "buy".to_string(),
            order_type: "limit".to_string(),
            quantity: 1.0,
            limit_price: Some(49000.0),
            stop_price: None,
            client_id: Some("client_456".to_string()),
        }
    }

    // =========================================================================
    // Position Tests
    // =========================================================================

    #[test]
    fn test_position_is_open() {
        let mut pos = create_test_position("BTCUSDT");

        pos.status = "open".to_string();
        assert!(pos.is_open());

        pos.status = "pending".to_string();
        assert!(pos.is_open());

        pos.status = "closing".to_string();
        assert!(pos.is_open());

        pos.status = "closed".to_string();
        assert!(!pos.is_open());
    }

    #[test]
    fn test_save_and_load_position() {
        let (manager, _temp) = create_test_manager();
        let pos = create_test_position("BTCUSDT");

        // Save position
        manager
            .save_position(&pos)
            .expect("Failed to save position");

        // Load all positions
        let positions = manager
            .load_positions(None)
            .expect("Failed to load positions");
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].symbol, "BTCUSDT");
        assert_eq!(positions[0].quantity, 1.5);
        assert_eq!(positions[0].entry_price, 50000.0);
    }

    #[test]
    fn test_load_positions_with_filter() {
        let (manager, _temp) = create_test_manager();

        // Save open position
        let mut pos1 = create_test_position("BTCUSDT");
        pos1.status = "open".to_string();
        manager.save_position(&pos1).unwrap();

        // Save closed position
        let mut pos2 = create_test_position("ETHUSDT");
        pos2.status = "closed".to_string();
        manager.save_position(&pos2).unwrap();

        // Filter by open
        let open = manager.load_positions(Some("open")).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].symbol, "BTCUSDT");

        // Filter by closed
        let closed = manager.load_positions(Some("closed")).unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].symbol, "ETHUSDT");

        // No filter
        let all = manager.load_positions(None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_get_position() {
        let (manager, _temp) = create_test_manager();
        let pos = create_test_position("BTCUSDT");
        manager.save_position(&pos).unwrap();

        // Get existing position
        let loaded = manager.get_position("BTCUSDT").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().entry_price, 50000.0);

        // Get non-existent position
        let missing = manager.get_position("XYZUSDT").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_position_update() {
        let (manager, _temp) = create_test_manager();

        // Save initial position
        let mut pos = create_test_position("BTCUSDT");
        manager.save_position(&pos).unwrap();

        // Update position
        pos.pnl = 500.0;
        pos.status = "closing".to_string();
        manager.save_position(&pos).unwrap();

        // Verify update
        let loaded = manager.get_position("BTCUSDT").unwrap().unwrap();
        assert_eq!(loaded.pnl, 500.0);
        assert_eq!(loaded.status, "closing");
    }

    #[test]
    fn test_position_with_metadata() {
        let (manager, _temp) = create_test_manager();

        let mut pos = create_test_position("BTCUSDT");
        pos.metadata
            .insert("trailing_stop".to_string(), serde_json::json!(49500.0));
        pos.metadata
            .insert("regime".to_string(), serde_json::json!("compression"));

        manager.save_position(&pos).unwrap();

        let loaded = manager.get_position("BTCUSDT").unwrap().unwrap();
        assert_eq!(loaded.metadata.get("trailing_stop").unwrap(), &49500.0);
        assert_eq!(loaded.metadata.get("regime").unwrap(), "compression");
    }

    // =========================================================================
    // Checkpoint Tests
    // =========================================================================

    #[test]
    fn test_save_and_load_checkpoint() {
        let (manager, _temp) = create_test_manager();
        let ckpt = create_test_checkpoint(42);

        manager
            .save_checkpoint(&ckpt)
            .expect("Failed to save checkpoint");

        let loaded = manager
            .load_checkpoint()
            .expect("Failed to load checkpoint");
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.cycle_count, 42);
        assert_eq!(loaded.portfolio_value, 10500.0);
        assert_eq!(loaded.cash, 5000.0);
        assert_eq!(loaded.open_positions, 2);
        assert!(loaded.paper_mode);
        assert_eq!(loaded.config_hash, "abc123");
    }

    #[test]
    fn test_load_latest_checkpoint() {
        let (manager, _temp) = create_test_manager();

        // Save multiple checkpoints
        for i in 1..=5 {
            let ckpt = create_test_checkpoint(i);
            manager.save_checkpoint(&ckpt).unwrap();
        }

        // Should load the latest (cycle 5)
        let loaded = manager.load_checkpoint().unwrap().unwrap();
        assert_eq!(loaded.cycle_count, 5);
    }

    #[test]
    fn test_load_checkpoint_empty() {
        let (manager, _temp) = create_test_manager();

        // No checkpoints saved
        let loaded = manager.load_checkpoint().unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_checkpoint_with_symbols() {
        let (manager, _temp) = create_test_manager();

        let mut ckpt = create_test_checkpoint(1);
        ckpt.last_processed_symbols = vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "SOLUSDT".to_string(),
        ];

        manager.save_checkpoint(&ckpt).unwrap();

        let loaded = manager.load_checkpoint().unwrap().unwrap();
        assert_eq!(loaded.last_processed_symbols.len(), 3);
        assert!(loaded
            .last_processed_symbols
            .contains(&"SOLUSDT".to_string()));
    }

    // =========================================================================
    // Trade Record Tests
    // =========================================================================

    #[test]
    fn test_record_trade() {
        let (manager, _temp) = create_test_manager();
        let trade = create_test_trade("BTCUSDT");

        manager
            .record_trade(&trade)
            .expect("Failed to record trade");

        // Verify by checking the database directly
        let conn = manager.conn.lock().unwrap();
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM trades", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_record_multiple_trades() {
        let (manager, _temp) = create_test_manager();

        for i in 0..5 {
            let mut trade = create_test_trade(&format!("SYMBOL{}", i));
            trade.net_pnl = if i % 2 == 0 { 100.0 } else { -50.0 };
            manager.record_trade(&trade).unwrap();
        }

        let conn = manager.conn.lock().unwrap();
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM trades", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 5);
    }

    // =========================================================================
    // Pending Order Tests
    // =========================================================================

    #[test]
    fn test_save_and_load_pending_order() {
        let (manager, _temp) = create_test_manager();
        let order = create_test_pending_order("ord_001", "BTCUSDT");

        manager
            .save_pending_order(&order)
            .expect("Failed to save order");

        let orders = manager
            .load_pending_orders()
            .expect("Failed to load orders");
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "ord_001");
        assert_eq!(orders[0].symbol, "BTCUSDT");
        assert_eq!(orders[0].limit_price, Some(49000.0));
    }

    #[test]
    fn test_multiple_pending_orders() {
        let (manager, _temp) = create_test_manager();

        manager
            .save_pending_order(&create_test_pending_order("ord_001", "BTCUSDT"))
            .unwrap();
        manager
            .save_pending_order(&create_test_pending_order("ord_002", "ETHUSDT"))
            .unwrap();
        manager
            .save_pending_order(&create_test_pending_order("ord_003", "SOLUSDT"))
            .unwrap();

        let orders = manager.load_pending_orders().unwrap();
        assert_eq!(orders.len(), 3);
    }

    #[test]
    fn test_update_pending_order() {
        let (manager, _temp) = create_test_manager();

        let mut order = create_test_pending_order("ord_001", "BTCUSDT");
        manager.save_pending_order(&order).unwrap();

        // Update the order (same ID)
        order.quantity = 2.5;
        order.limit_price = Some(48000.0);
        manager.save_pending_order(&order).unwrap();

        let orders = manager.load_pending_orders().unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].quantity, 2.5);
        assert_eq!(orders[0].limit_price, Some(48000.0));
    }

    #[test]
    fn test_remove_pending_order() {
        let (manager, _temp) = create_test_manager();

        manager
            .save_pending_order(&create_test_pending_order("ord_001", "BTCUSDT"))
            .unwrap();
        manager
            .save_pending_order(&create_test_pending_order("ord_002", "ETHUSDT"))
            .unwrap();

        // Remove one order
        manager.remove_pending_order("ord_001").unwrap();

        let orders = manager.load_pending_orders().unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "ord_002");
    }

    #[test]
    fn test_clear_pending_orders() {
        let (manager, _temp) = create_test_manager();

        for i in 0..5 {
            manager
                .save_pending_order(&create_test_pending_order(&format!("ord_{}", i), "BTCUSDT"))
                .unwrap();
        }

        assert_eq!(manager.load_pending_orders().unwrap().len(), 5);

        manager.clear_pending_orders().unwrap();

        assert_eq!(manager.load_pending_orders().unwrap().len(), 0);
    }

    #[test]
    fn test_pending_order_with_stop_price() {
        let (manager, _temp) = create_test_manager();

        let order = PendingOrder {
            order_id: "stop_001".to_string(),
            symbol: "BTCUSDT".to_string(),
            side: "sell".to_string(),
            order_type: "stop".to_string(),
            quantity: 1.0,
            limit_price: None,
            stop_price: Some(47000.0),
            client_id: None,
        };

        manager.save_pending_order(&order).unwrap();

        let loaded = manager.load_pending_orders().unwrap();
        assert_eq!(loaded[0].stop_price, Some(47000.0));
        assert_eq!(loaded[0].limit_price, None);
    }

    // =========================================================================
    // JSON Export Tests
    // =========================================================================

    #[test]
    fn test_export_json() {
        let (manager, temp) = create_test_manager();

        // Add some data
        manager
            .save_position(&create_test_position("BTCUSDT"))
            .unwrap();
        manager.save_checkpoint(&create_test_checkpoint(1)).unwrap();
        manager
            .save_pending_order(&create_test_pending_order("ord_001", "BTCUSDT"))
            .unwrap();

        // Export
        manager.export_json().expect("Failed to export JSON");

        // Verify file exists and contains data
        let json_path = temp.path().join("test.json");
        assert!(json_path.exists());

        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert!(parsed.get("positions").is_some());
        assert!(parsed.get("checkpoint").is_some());
        assert!(parsed.get("pending_orders").is_some());
        assert!(parsed.get("exported_at").is_some());
    }

    #[test]
    fn test_export_json_empty() {
        let (manager, temp) = create_test_manager();

        // Export with no data
        manager.export_json().unwrap();

        let json_path = temp.path().join("test.json");
        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert!(parsed["positions"].as_array().unwrap().is_empty());
        assert!(parsed["checkpoint"].is_null());
        assert!(parsed["pending_orders"].as_array().unwrap().is_empty());
    }

    // =========================================================================
    // Factory Function Tests
    // =========================================================================

    #[test]
    fn test_create_state_manager() {
        let temp_dir = TempDir::new().unwrap();
        let state_dir = temp_dir.path().join("state");

        let manager = create_state_manager(&state_dir, "sqlite").expect("Failed to create manager");

        // Verify directory structure
        assert!(state_dir.exists());
        assert!(state_dir.join("trading_state.db").exists());

        // Verify manager works
        manager
            .save_position(&create_test_position("TEST"))
            .unwrap();
        let pos = manager.get_position("TEST").unwrap();
        assert!(pos.is_some());
    }

    // =========================================================================
    // Edge Cases and Error Handling
    // =========================================================================

    #[test]
    fn test_position_with_special_characters() {
        let (manager, _temp) = create_test_manager();

        let mut pos = create_test_position("BTC/USDT:PERP");
        pos.order_id = Some("order_with_special_chars_!@#$%".to_string());
        manager.save_position(&pos).unwrap();

        let loaded = manager.get_position("BTC/USDT:PERP").unwrap().unwrap();
        assert_eq!(
            loaded.order_id,
            Some("order_with_special_chars_!@#$%".to_string())
        );
    }

    #[test]
    fn test_position_with_zero_values() {
        let (manager, _temp) = create_test_manager();

        let pos = Position {
            symbol: "BTCUSDT".to_string(),
            side: "buy".to_string(),
            quantity: 0.0,
            entry_price: 0.0,
            entry_time: None,
            stop_loss: 0.0,
            take_profit: 0.0,
            status: "pending".to_string(),
            order_id: None,
            pnl: 0.0,
            exit_price: 0.0,
            exit_time: None,
            metadata: HashMap::new(),
        };

        manager.save_position(&pos).unwrap();
        let loaded = manager.get_position("BTCUSDT").unwrap().unwrap();
        assert_eq!(loaded.quantity, 0.0);
        assert_eq!(loaded.entry_price, 0.0);
    }

    #[test]
    fn test_large_metadata() {
        let (manager, _temp) = create_test_manager();

        let mut pos = create_test_position("BTCUSDT");
        // Add many metadata entries
        for i in 0..100 {
            pos.metadata.insert(
                format!("key_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        manager.save_position(&pos).unwrap();
        let loaded = manager.get_position("BTCUSDT").unwrap().unwrap();
        assert_eq!(loaded.metadata.len(), 100);
    }

    #[test]
    fn test_concurrent_access_simulation() {
        let (manager, _temp) = create_test_manager();

        // Simulate multiple saves in quick succession
        for i in 0..10 {
            let pos = create_test_position(&format!("SYM{}", i));
            manager.save_position(&pos).unwrap();
        }

        for i in 0..5 {
            let ckpt = create_test_checkpoint(i);
            manager.save_checkpoint(&ckpt).unwrap();
        }

        // Verify all data is intact
        let positions = manager.load_positions(None).unwrap();
        assert_eq!(positions.len(), 10);

        let ckpt = manager.load_checkpoint().unwrap().unwrap();
        assert_eq!(ckpt.cycle_count, 4); // Latest
    }
}
