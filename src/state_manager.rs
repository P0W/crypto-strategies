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
    pub side: String,  // "buy" or "sell"
    pub quantity: f64,
    pub entry_price: f64,
    pub entry_time: Option<String>,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub status: String,  // "pending", "open", "closing", "closed"
    pub order_id: Option<String>,
    pub pnl: f64,
    pub exit_price: f64,
    pub exit_time: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Position {
    pub fn is_open(&self) -> bool {
        matches!(
            self.status.as_str(),
            "open" | "pending" | "closing"
        )
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

// =============================================================================
// State Manager Implementation
// =============================================================================

pub struct SqliteStateManager {
    conn: Arc<Mutex<Connection>>,
    json_backup_path: PathBuf,
    auto_backup: bool,
}

impl SqliteStateManager {
    pub fn new<P: AsRef<Path>>(
        db_path: P,
        json_backup_path: P,
        auto_backup: bool,
    ) -> Result<Self> {
        let db_path = db_path.as_ref();
        
        // Create parent directories
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = json_backup_path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let manager = Self {
            conn: Arc::new(Mutex::new(conn)),
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
                last_processed_symbols: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
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

    pub fn export_json(&self) -> Result<()> {
        let positions = self.load_positions(None)?;
        let checkpoint = self.load_checkpoint()?;

        let state = serde_json::json!({
            "exported_at": Utc::now().to_rfc3339(),
            "positions": positions,
            "checkpoint": checkpoint,
        });

        std::fs::write(&self.json_backup_path, serde_json::to_string_pretty(&state)?)?;
        debug!("State exported to: {}", self.json_backup_path.display());
        Ok(())
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
