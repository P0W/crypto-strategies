//! Async market-data and persistence adapter over the shared trading core.

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Duration, Utc};
use crypto_strategies::coindcx::{ClientConfig, CoinDCXClient};
use crypto_strategies::data::{candle_close_time, timeframe_duration};
use crypto_strategies::oms::broker::MarketRules;
use crypto_strategies::oms::live_execution::LiveExecution;
use crypto_strategies::oms::{EngineSnapshot, TradingEngine};
use crypto_strategies::state_manager::SqliteStateManager;
use crypto_strategies::{
    strategies, Candle, Config, MultiSymbolMultiTimeframeData, MultiTimeframeData, Symbol,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tracing::{info, warn};

const HISTORY_BARS: u32 = 500;
const SNAPSHOT_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct PaperSnapshot {
    version: u32,
    config: serde_json::Value,
    engine: EngineSnapshot,
}

fn config_identity(config: &Config) -> Result<serde_json::Value> {
    let mut public_config = config.clone();
    public_config.exchange.api_key = None;
    public_config.exchange.api_secret = None;
    Ok(serde_json::to_value(public_config)?)
}

fn closed_history(mut candles: Vec<Candle>, tf: &str, now: DateTime<Utc>) -> Result<Vec<Candle>> {
    ensure!(
        timeframe_duration(tf).is_some(),
        "Unsupported timeframe {tf}"
    );
    candles.sort_by_key(|c| c.datetime);
    candles.dedup_by_key(|c| c.datetime);
    candles.retain(|c| candle_close_time(c.datetime, tf).is_some_and(|close| close <= now));
    let latest = candles.last().context("No closed candles")?;
    let close = candle_close_time(latest.datetime, tf).context("Invalid candle close")?;
    let next_close = candle_close_time(close, tf).context("Invalid candle interval")?;
    ensure!(
        now <= next_close + Duration::seconds(30),
        "Stale {tf} market data"
    );
    Ok(candles)
}

async fn fetch_frame(
    client: &CoinDCXClient,
    config: &Config,
    timeframes: &[String],
    rules: Option<&BTreeMap<String, MarketRules>>,
) -> Result<MultiSymbolMultiTimeframeData> {
    let now = Utc::now();
    let mut data = MultiSymbolMultiTimeframeData::new();
    for name in &config.trading.symbols {
        let mut mtf = MultiTimeframeData::new(config.timeframe());
        for tf in timeframes {
            let pair = match rules {
                Some(rules) => rules
                    .get(name)
                    .context("Missing verified candle pair")?
                    .data_pair
                    .as_str(),
                None => name,
            };
            let raw = client
                .get_candles(pair, tf, Some(HISTORY_BARS))
                .await
                .with_context(|| format!("Fetching {name} {tf} candles"))?;
            let candles = raw
                .into_iter()
                .map(Candle::try_from)
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("Invalid {name} {tf} candle"))?;
            mtf.add_timeframe(tf, closed_history(candles, tf, now)?);
        }
        data.insert(Symbol::new(name), mtf);
    }
    Ok(data)
}

fn replay_dates(
    data: &MultiSymbolMultiTimeframeData,
    last: Option<DateTime<Utc>>,
    latest: DateTime<Utc>,
) -> Result<Vec<DateTime<Utc>>> {
    let Some(last) = last else {
        return Ok(vec![latest]);
    };
    if latest <= last {
        return Ok(vec![]);
    }
    let timeframe = data
        .values()
        .next()
        .context("No market data")?
        .primary_timeframe();
    let mut expected = Vec::new();
    let mut next = candle_close_time(last, timeframe).context("Invalid replay interval")?;
    while next <= latest {
        expected.push(next);
        next = candle_close_time(next, timeframe).context("Invalid replay interval")?;
    }
    ensure!(
        expected.last() == Some(&latest),
        "Latest candle is not aligned with the recovery interval"
    );
    for (symbol, history) in data {
        ensure!(
            history.primary_timeframe() == timeframe,
            "Primary timeframes differ"
        );
        ensure!(
            history.primary().iter().any(|c| c.datetime == last),
            "Data gap exceeds recovery history for {symbol}; refusing to skip unprocessed candles"
        );
        ensure!(
            history.primary().iter().filter(|c| c.datetime > last).map(|c| c.datetime)
                .eq(expected.iter().copied()),
            "Missing or mismatched replay candles for {symbol}; refusing to skip unprocessed candles"
        );
    }
    Ok(expected)
}

fn restore(
    engine: &mut TradingEngine,
    db: &SqliteStateManager,
    identity: &serde_json::Value,
) -> Result<()> {
    if let Some(snapshot) = db.load_engine_snapshot::<PaperSnapshot>()? {
        ensure!(
            snapshot.version == SNAPSHOT_VERSION,
            "Unsupported snapshot version"
        );
        ensure!(
            &snapshot.config == identity,
            "State/config mismatch; use a separate state database for a different configuration"
        );
        engine.restore(snapshot.engine)?;
    } else {
        ensure!(
            !db.has_legacy_state()?,
            "Legacy state cannot be recovered safely. Reconcile any real exchange orders \
             manually and use a new paper-state database; no legacy positions or orders were migrated."
        );
    }
    Ok(())
}

async fn persist(
    engine: &TradingEngine,
    db: &SqliteStateManager,
    identity: &serde_json::Value,
) -> Result<()> {
    let snapshot = PaperSnapshot {
        version: SNAPSHOT_VERSION,
        config: identity.clone(),
        engine: engine.snapshot()?,
    };
    let db = db.clone();
    tokio::task::spawn_blocking(move || db.save_engine_snapshot(&snapshot))
        .await
        .context("Persistence worker failed")??;
    Ok(())
}

pub async fn run(
    config: Config,
    state_db_path: String,
    paper_mode: bool,
    preflight: bool,
    resume: bool,
) -> Result<()> {
    ensure!(
        !paper_mode || (!preflight && !resume),
        "Preflight/resume apply only to real execution"
    );
    if !paper_mode {
        ensure!(
            config
                .exchange
                .api_key
                .as_deref()
                .is_some_and(|s| !s.is_empty()),
            "Missing COINDCX_API_KEY"
        );
        ensure!(
            config
                .exchange
                .api_secret
                .as_deref()
                .is_some_and(|s| !s.is_empty()),
            "Missing COINDCX_API_SECRET"
        );
    }
    ensure!(
        !config.trading.symbols.is_empty(),
        "No trading symbols configured"
    );
    ensure!(
        config.exchange.rate_limit > 0,
        "Rate limit must be positive"
    );
    let strategy = strategies::create_strategy(&config)?;
    let mut timeframes: Vec<String> = strategy
        .required_timeframes()
        .into_iter()
        .map(str::to_owned)
        .collect();
    timeframes.push(config.timeframe());
    timeframes.sort();
    timeframes.dedup();
    for tf in &timeframes {
        ensure!(
            timeframe_duration(tf).is_some(),
            "Unsupported timeframe {tf}"
        );
    }
    let identity = config_identity(&config)?;
    let db_path = Path::new(&state_db_path);
    if let Some(parent) = db_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let lease = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(db_path.with_extension("lock"))?;
    lease
        .try_lock()
        .context("This state database is already in use")?;
    let db = SqliteStateManager::new(db_path.to_path_buf(), db_path.with_extension("json"), true)?;
    if !paper_mode {
        let client = CoinDCXClient::with_config(
            config
                .exchange
                .api_key
                .as_deref()
                .filter(|s| !s.is_empty())
                .context("Missing COINDCX_API_KEY")?,
            config
                .exchange
                .api_secret
                .as_deref()
                .filter(|s| !s.is_empty())
                .context("Missing COINDCX_API_SECRET")?,
            ClientConfig::default()
                .with_rate_limit(config.exchange.rate_limit as usize)
                .with_timeout(std::time::Duration::from_secs(10)),
        );
        let mut live = LiveExecution::open(config.clone(), strategy, client.clone(), db).await?;
        if preflight {
            live.preflight().await?;
            info!("Read-only preflight passed: market capabilities, account ownership and balances reconcile; no orders submitted");
            return Ok(());
        }
        if let Err(error) = live.preflight().await {
            live.halt(format!("Startup reconciliation failed: {error:#}"))
                .await?;
        }
        if resume {
            live.resume().await?;
        }
        let halt_path = db_path.with_extension("halt");
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        info!("CoinDCX spot execution enabled; native stop-limit orders do not guarantee fills through price gaps");
        loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => {
                    signal.context("Failed to receive shutdown signal")?;
                    break;
                }
                _ = ticker.tick() => {}
            }
            if halt_path.try_exists()? && live.halted().is_none() {
                live.halt("Operator halt file").await?;
            }
            if let Err(error) = live.cycle().await {
                live.halt(format!("Live cycle failed: {error:#}")).await?;
                continue;
            }
            if live.halted().is_some() {
                continue;
            }
            let data = match fetch_frame(&client, &config, &timeframes, Some(live.rules())).await {
                Ok(data) => data,
                Err(error) => {
                    live.halt(format!("Market data failed: {error:#}")).await?;
                    continue;
                }
            };
            let dates: Vec<_> = data
                .values()
                .filter_map(|d| d.primary().last())
                .map(|c| c.datetime)
                .collect();
            let date = *dates.first().context("No live candles")?;
            if dates.iter().any(|d| *d != date) {
                live.halt("Unsynchronized market data").await?;
                continue;
            }
            if live.engine().last_bar().is_none_or(|last| date > last) {
                // Real execution never replays missed historical signals as new orders.
                if let Err(error) = live.on_bar(&data, date).await {
                    live.halt(format!("Live decision failed: {error:#}"))
                        .await?;
                    continue;
                }
            }
        }
        live.prepare_shutdown().await?;
        for _ in 0..15 {
            if let Err(error) = live.cycle().await {
                warn!("Shutdown reconciliation failed: {error:#}");
            }
            if live.shutdown_ready() {
                info!("Entries cancelled; open inventory retains acknowledged native stops. State is halted until --resume.");
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        anyhow::bail!("Shutdown could not confirm all cancellations/protection. Live state is halted; inspect venue orders/inventory immediately.");
    }
    let mut engine = TradingEngine::new(config.clone(), strategy)?;
    restore(&mut engine, &db, &identity)?;
    let client = CoinDCXClient::with_config(
        "",
        "",
        ClientConfig::default()
            .with_rate_limit(config.exchange.rate_limit as usize)
            .with_timeout(std::time::Duration::from_secs(10)),
    );
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    info!("Paper execution uses the same trading engine and execution policy as backtest");

    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.context("Failed to receive shutdown signal")?;
                break;
            }
            _ = ticker.tick() => {}
        }
        let data = match fetch_frame(&client, &config, &timeframes, None).await {
            Ok(data) => data,
            Err(error) => {
                warn!("Skipping entire frame; market data unavailable: {error:#}");
                continue;
            }
        };
        let latest: Vec<_> = data
            .values()
            .filter_map(|mtf| mtf.primary().last())
            .map(|c| c.datetime)
            .collect();
        let timestamp = *latest.first().context("No primary candles")?;
        if latest.iter().any(|date| *date != timestamp) {
            warn!("Skipping frame: symbols do not have the same latest closed candle");
            continue;
        }
        let dates = replay_dates(&data, engine.last_bar(), timestamp)?;
        for date in dates {
            engine
                .on_bar(&data, date)
                .context("Trading engine failed; stopping paper trading")?;
            persist(&engine, &db, &identity)
                .await
                .context("State persistence failed; trading stopped")?;
            info!(%date, equity = engine.equity(), cash = engine.cash(),
                positions = engine.positions().open_position_count(),
                drawdown = engine.risk().current_drawdown(), "Paper frame committed");
        }
    }
    persist(&engine, &db, &identity).await?;
    info!("Paper engine stopped; pending orders and positions remain in the atomic snapshot");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn replay_requires_every_expected_bar_for_every_symbol() {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        for (a, b, valid) in [
            (vec![0, 1, 2, 3], vec![0, 1, 2, 3], true),
            (vec![0, 1, 3], vec![0, 1, 3], false),
            (vec![0, 1, 2, 3], vec![0, 1, 3], false),
            (vec![0, 1, 3], vec![0, 1, 2, 3], false),
            (vec![2, 3], vec![0, 1, 2, 3], false),
        ] {
            let mut frame = MultiSymbolMultiTimeframeData::new();
            for (symbol, hours) in [("A", a), ("B", b)] {
                let mut history = MultiTimeframeData::new("1h");
                history.add_timeframe(
                    "1h",
                    hours
                        .into_iter()
                        .map(|h| {
                            Candle::new(start + Duration::hours(h), 100.0, 100.0, 100.0, 100.0, 1.0)
                                .unwrap()
                        })
                        .collect(),
                );
                frame.insert(Symbol::new(symbol), history);
            }
            let dates = replay_dates(
                &frame,
                Some(start + Duration::hours(1)),
                start + Duration::hours(3),
            );
            if valid {
                assert_eq!(
                    dates.unwrap(),
                    vec![start + Duration::hours(2), start + Duration::hours(3)]
                );
            } else {
                assert!(dates.is_err());
            }
        }
    }

    #[test]
    fn monthly_replay_uses_calendar_boundaries() {
        let dates: Vec<_> = (1..=3)
            .map(|month| Utc.with_ymd_and_hms(2026, month, 1, 0, 0, 0).unwrap())
            .collect();
        let mut history = MultiTimeframeData::new("1M");
        history.add_timeframe(
            "1M",
            dates
                .iter()
                .map(|date| Candle::new(*date, 100.0, 100.0, 100.0, 100.0, 1.0).unwrap())
                .collect(),
        );
        let frame = MultiSymbolMultiTimeframeData::from([(Symbol::new("TEST"), history)]);
        assert_eq!(
            replay_dates(&frame, Some(dates[0]), dates[2]).unwrap(),
            dates[1..]
        );
    }

    #[tokio::test]
    async fn missing_live_credentials_are_rejected_before_any_side_effect() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("configs")
            .join("sample_config.json");
        let config = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("must-not-exist.db");
        let error = run(
            config,
            state.to_str().unwrap().to_owned(),
            false,
            false,
            false,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Missing COINDCX_API_KEY"));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn newest_first_history_is_sorted_and_unfinished_bar_is_excluded() {
        let now = Utc.with_ymd_and_hms(2026, 1, 2, 12, 30, 0).unwrap();
        let candles = [12, 11, 10]
            .into_iter()
            .map(|hour| {
                Candle::new(
                    now.date_naive().and_hms_opt(hour, 0, 0).unwrap().and_utc(),
                    100.0,
                    100.0,
                    100.0,
                    100.0,
                    1.0,
                )
                .unwrap()
            })
            .collect();
        let history = closed_history(candles, "1h", now).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history[0].datetime < history[1].datetime);
        assert_eq!(
            candle_close_time(history[1].datetime, "1h").unwrap(),
            now - Duration::minutes(30)
        );
    }

    #[test]
    fn stale_or_empty_history_is_rejected() {
        let now = Utc::now();
        let old = Candle::new(now - Duration::days(2), 100.0, 100.0, 100.0, 100.0, 1.0).unwrap();
        assert!(closed_history(vec![old], "1h", now).is_err());
        assert!(closed_history(vec![], "1h", now).is_err());
    }
}
