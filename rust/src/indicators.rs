//! Technical indicators powered by the `ta` crate
//!
//! This module provides wrappers around the battle-tested `ta` crate for technical analysis.
//! The `ta` crate is widely used (~179K downloads), well-maintained, and has minimal dependencies.
//!
//! Available indicators:
//! - Moving Averages: SMA, EMA
//! - Momentum: RSI, Stochastic, MACD
//! - Volatility: ATR, Bollinger Bands, Keltner Channels
//! - Volume: OBV, MFI
//! - Other: CCI, Standard Deviation

use std::collections::HashMap;
use ta::indicators::{
    BollingerBands as TaBB, CommodityChannelIndex, ExponentialMovingAverage, FastStochastic,
    KeltnerChannel, MoneyFlowIndex as TaMFI, MovingAverageConvergenceDivergence, OnBalanceVolume,
    RelativeStrengthIndex, SimpleMovingAverage,
};
use ta::{DataItem, Next};

// =============================================================================
// Type Aliases for Complex Return Types
// =============================================================================

/// Type alias for band indicators (upper, middle, lower)
pub type BandOutput = (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>);

/// Type alias for two-line indicators (line1, line2)
pub type DualLineOutput = (Vec<Option<f64>>, Vec<Option<f64>>);

// =============================================================================
// Data Item Helper
// =============================================================================

/// Create a DataItem from OHLCV data for use with ta indicators
pub fn make_data_item(open: f64, high: f64, low: f64, close: f64, volume: f64) -> DataItem {
    DataItem::builder()
        .open(open)
        .high(high)
        .low(low)
        .close(close)
        .volume(volume)
        .build()
        .unwrap()
}

// =============================================================================
// Moving Averages
// =============================================================================

/// Calculate Simple Moving Average
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if values.is_empty() || period == 0 {
        return vec![];
    }

    let mut indicator = match SimpleMovingAverage::new(period) {
        Ok(i) => i,
        Err(_) => return vec![None; values.len()],
    };

    let mut result = Vec::with_capacity(values.len());

    for (i, &value) in values.iter().enumerate() {
        let sma_val = indicator.next(value);
        if i + 1 >= period {
            result.push(Some(sma_val));
        } else {
            result.push(None);
        }
    }

    result
}

/// Calculate Exponential Moving Average
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if values.is_empty() || period == 0 {
        return vec![];
    }

    let mut indicator = match ExponentialMovingAverage::new(period) {
        Ok(i) => i,
        Err(_) => return vec![None; values.len()],
    };

    let mut result = Vec::with_capacity(values.len());

    for (i, &value) in values.iter().enumerate() {
        let ema_val = indicator.next(value);
        if i + 1 >= period {
            result.push(Some(ema_val));
        } else {
            result.push(None);
        }
    }

    result
}

/// Calculate Weighted Moving Average (manual implementation - not in ta crate)
pub fn wma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if values.is_empty() || period == 0 {
        return vec![];
    }

    let mut result = Vec::with_capacity(values.len());
    let weight_sum: f64 = (1..=period).map(|x| x as f64).sum();

    for i in 0..values.len() {
        if i + 1 < period {
            result.push(None);
        } else {
            let weighted_sum: f64 = values[i + 1 - period..=i]
                .iter()
                .enumerate()
                .map(|(j, &v)| v * (j + 1) as f64)
                .sum();
            result.push(Some(weighted_sum / weight_sum));
        }
    }

    result
}

/// Calculate Hull Moving Average (HMA)
/// HMA = WMA(2 * WMA(n/2) - WMA(n), sqrt(n))
pub fn hma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if values.is_empty() || period < 2 {
        return vec![];
    }

    let half_period = period / 2;
    let sqrt_period = (period as f64).sqrt() as usize;

    let wma_half = wma(values, half_period);
    let wma_full = wma(values, period);

    // Calculate 2 * WMA(n/2) - WMA(n)
    let mut raw_hma = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        match (wma_half.get(i), wma_full.get(i)) {
            (Some(Some(half)), Some(Some(full))) => raw_hma.push(2.0 * half - full),
            _ => raw_hma.push(value), // Use original value if not enough data
        }
    }

    wma(&raw_hma, sqrt_period.max(1))
}

// =============================================================================
// Volatility Indicators
// =============================================================================

/// Calculate True Range
pub fn true_range(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let mut tr = Vec::with_capacity(high.len());

    for i in 0..high.len() {
        let tr_value = if i == 0 {
            high[i] - low[i]
        } else {
            let hl = high[i] - low[i];
            let hc = (high[i] - close[i - 1]).abs();
            let lc = (low[i] - close[i - 1]).abs();
            hl.max(hc).max(lc)
        };
        tr.push(tr_value);
    }

    tr
}

/// Calculate Average True Range (ATR) using Wilder's smoothing
///
/// This uses Wilder's smoothing method (same as backtrader) where:
/// ATR = (prev_ATR * (period - 1) + current_TR) / period
///
/// This is equivalent to EMA with smoothing factor = 1/period instead of 2/(period+1)
pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    if high.is_empty() || period == 0 || high.len() != low.len() || high.len() != close.len() {
        return vec![];
    }

    let tr = true_range(high, low, close);
    let mut result = Vec::with_capacity(high.len());

    // Wilder's smoothing: ATR = (prev_ATR * (period - 1) + current_TR) / period
    let mut atr_value: Option<f64> = None;

    for i in 0..tr.len() {
        if i + 1 < period {
            // Not enough data yet
            result.push(None);
        } else if i + 1 == period {
            // First ATR value is SMA of first `period` TR values
            let sum: f64 = tr[0..period].iter().sum();
            atr_value = Some(sum / period as f64);
            result.push(atr_value);
        } else {
            // Wilder's smoothing for subsequent values
            if let Some(prev_atr) = atr_value {
                let new_atr = (prev_atr * (period - 1) as f64 + tr[i]) / period as f64;
                atr_value = Some(new_atr);
                result.push(atr_value);
            } else {
                result.push(None);
            }
        }
    }

    result
}

/// Calculate ATR as percentage of price
pub fn atr_percent(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let atr_vals = atr(high, low, close, period);

    atr_vals
        .iter()
        .zip(close.iter())
        .map(|(atr_opt, &price)| {
            atr_opt.map(|atr_val| {
                if price > 0.0 {
                    (atr_val / price) * 100.0
                } else {
                    0.0
                }
            })
        })
        .collect()
}

/// Bollinger Bands result from ta crate
pub struct BollingerBandsOutput {
    pub average: f64,
    pub upper: f64,
    pub lower: f64,
}

/// Calculate Bollinger Bands using ta crate
pub fn bollinger_bands(values: &[f64], period: usize, num_std: f64) -> BandOutput {
    if values.is_empty() || period == 0 {
        return (vec![], vec![], vec![]);
    }

    let mut indicator = match TaBB::new(period, num_std) {
        Ok(i) => i,
        Err(_) => {
            return (
                vec![None; values.len()],
                vec![None; values.len()],
                vec![None; values.len()],
            )
        }
    };

    let mut upper = Vec::with_capacity(values.len());
    let mut middle = Vec::with_capacity(values.len());
    let mut lower = Vec::with_capacity(values.len());

    for (i, &value) in values.iter().enumerate() {
        let bb = indicator.next(value);
        if i + 1 >= period {
            upper.push(Some(bb.upper));
            middle.push(Some(bb.average));
            lower.push(Some(bb.lower));
        } else {
            upper.push(None);
            middle.push(None);
            lower.push(None);
        }
    }

    (upper, middle, lower)
}

/// Calculate Keltner Channels using ta crate
/// Note: ta crate uses same period for EMA and ATR
pub fn keltner_channels(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    period: usize,
    _atr_period: usize, // Ignored - ta uses same period for both
    atr_multiplier: f64,
) -> BandOutput {
    if high.is_empty() || high.len() != low.len() || high.len() != close.len() {
        return (vec![], vec![], vec![]);
    }

    let mut indicator = match KeltnerChannel::new(period, atr_multiplier) {
        Ok(i) => i,
        Err(_) => {
            return (
                vec![None; high.len()],
                vec![None; high.len()],
                vec![None; high.len()],
            )
        }
    };

    let mut upper = Vec::with_capacity(high.len());
    let mut middle = Vec::with_capacity(high.len());
    let mut lower = Vec::with_capacity(high.len());

    for i in 0..high.len() {
        let item = make_data_item(close[i], high[i], low[i], close[i], 0.0);
        let kc = indicator.next(&item);
        if i + 1 >= period {
            upper.push(Some(kc.upper));
            middle.push(Some(kc.average));
            lower.push(Some(kc.lower));
        } else {
            upper.push(None);
            middle.push(None);
            lower.push(None);
        }
    }

    (upper, middle, lower)
}

// =============================================================================
// Momentum Indicators
// =============================================================================

/// Calculate RSI (Relative Strength Index) using ta crate
pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if values.is_empty() || period == 0 {
        return vec![];
    }

    let mut indicator = match RelativeStrengthIndex::new(period) {
        Ok(i) => i,
        Err(_) => return vec![None; values.len()],
    };

    let mut result = Vec::with_capacity(values.len());

    for (i, &value) in values.iter().enumerate() {
        let rsi_val = indicator.next(value);
        if i + 1 >= period {
            result.push(Some(rsi_val));
        } else {
            result.push(None);
        }
    }

    result
}

/// Calculate Stochastic Oscillator using ta crate
/// Returns (%K, %D) where %D is SMA of %K
pub fn stochastic(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    k_period: usize,
    d_period: usize,
) -> DualLineOutput {
    if high.is_empty() || high.len() != low.len() || high.len() != close.len() {
        return (vec![], vec![]);
    }

    // FastStochastic gives us %K, we'll compute %D as SMA of %K
    let mut fast_stoch = match FastStochastic::new(k_period) {
        Ok(i) => i,
        Err(_) => return (vec![None; high.len()], vec![None; high.len()]),
    };

    let mut k_values = Vec::with_capacity(high.len());
    let mut raw_k = Vec::new();

    for i in 0..high.len() {
        let item = make_data_item(close[i], high[i], low[i], close[i], 0.0);
        let k_val = fast_stoch.next(&item);
        if i + 1 >= k_period {
            k_values.push(Some(k_val));
            raw_k.push(k_val);
        } else {
            k_values.push(None);
        }
    }

    // Calculate %D as SMA of %K
    let d_sma = sma(&raw_k, d_period);
    let padding = high.len() - d_sma.len();
    let mut d_values: Vec<Option<f64>> = vec![None; padding];
    d_values.extend(d_sma);

    (k_values, d_values)
}

/// Calculate Fast Stochastic (unsmoothed %K with SMA %D)
pub fn fast_stochastic(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    k_period: usize,
    d_period: usize,
) -> DualLineOutput {
    if high.is_empty() || high.len() != low.len() || high.len() != close.len() {
        return (vec![], vec![]);
    }

    let mut indicator = match FastStochastic::new(k_period) {
        Ok(i) => i,
        Err(_) => return (vec![None; high.len()], vec![None; high.len()]),
    };

    let mut k_values = Vec::with_capacity(high.len());
    let mut raw_k = Vec::new();

    for i in 0..high.len() {
        let item = make_data_item(close[i], high[i], low[i], close[i], 0.0);
        let k_val = indicator.next(&item);
        if i + 1 >= k_period {
            k_values.push(Some(k_val));
            raw_k.push(k_val);
        } else {
            k_values.push(None);
        }
    }

    // Calculate %D as SMA of %K
    let d_sma = sma(&raw_k, d_period);
    let padding = high.len() - d_sma.len();
    let mut d_values: Vec<Option<f64>> = vec![None; padding];
    d_values.extend(d_sma);

    (k_values, d_values)
}

/// Calculate MACD using ta crate
pub fn macd(
    values: &[f64],
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> BandOutput {
    if values.is_empty() {
        return (vec![], vec![], vec![]);
    }

    let mut indicator =
        match MovingAverageConvergenceDivergence::new(fast_period, slow_period, signal_period) {
            Ok(i) => i,
            Err(_) => {
                return (
                    vec![None; values.len()],
                    vec![None; values.len()],
                    vec![None; values.len()],
                )
            }
        };

    let warmup = slow_period;
    let mut macd_line = Vec::with_capacity(values.len());
    let mut signal_line = Vec::with_capacity(values.len());
    let mut histogram = Vec::with_capacity(values.len());

    for (i, &value) in values.iter().enumerate() {
        let macd_out = indicator.next(value);
        if i + 1 >= warmup {
            macd_line.push(Some(macd_out.macd));
            signal_line.push(Some(macd_out.signal));
            histogram.push(Some(macd_out.histogram));
        } else {
            macd_line.push(None);
            signal_line.push(None);
            histogram.push(None);
        }
    }

    (macd_line, signal_line, histogram)
}

/// Calculate Williams %R (manual - similar to Stochastic but inverted scale)
pub fn williams_r(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    if high.is_empty() || period == 0 {
        return vec![];
    }

    let mut result = Vec::with_capacity(close.len());

    for i in 0..close.len() {
        if i + 1 < period {
            result.push(None);
        } else {
            let window_high: f64 = high[i + 1 - period..=i]
                .iter()
                .fold(f64::MIN, |a, &b| a.max(b));
            let window_low: f64 = low[i + 1 - period..=i]
                .iter()
                .fold(f64::MAX, |a, &b| a.min(b));

            let range = window_high - window_low;
            if range > 0.0 {
                let wr = ((window_high - close[i]) / range) * -100.0;
                result.push(Some(wr));
            } else {
                result.push(Some(-50.0));
            }
        }
    }

    result
}

/// Calculate Commodity Channel Index (CCI) using ta crate
pub fn cci(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    if high.is_empty() || high.len() != low.len() || high.len() != close.len() {
        return vec![];
    }

    let mut indicator = match CommodityChannelIndex::new(period) {
        Ok(i) => i,
        Err(_) => return vec![None; high.len()],
    };

    let mut result = Vec::with_capacity(high.len());

    for i in 0..high.len() {
        let item = make_data_item(close[i], high[i], low[i], close[i], 0.0);
        let cci_val = indicator.next(&item);
        if i + 1 >= period {
            result.push(Some(cci_val));
        } else {
            result.push(None);
        }
    }

    result
}

// =============================================================================
// Trend Indicators
// =============================================================================

/// Apply Wilder's smoothing to a series
/// Wilder's smoothing: new_value = (prev_value * (period - 1) + current) / period
fn wilders_smooth(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if values.is_empty() || period == 0 {
        return vec![];
    }

    let mut result = Vec::with_capacity(values.len());
    let mut smoothed: Option<f64> = None;

    for i in 0..values.len() {
        if i + 1 < period {
            result.push(None);
        } else if i + 1 == period {
            // First value is SMA
            let sum: f64 = values[0..period].iter().sum();
            smoothed = Some(sum / period as f64);
            result.push(smoothed);
        } else if let Some(prev) = smoothed {
            let new_val = (prev * (period - 1) as f64 + values[i]) / period as f64;
            smoothed = Some(new_val);
            result.push(smoothed);
        } else {
            result.push(None);
        }
    }

    result
}

/// Calculate Directional Movement Index (DMI) components using Wilder's smoothing
/// Returns (+DI, -DI)
pub fn dmi(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    period: usize,
) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    if high.is_empty() || period == 0 {
        return (vec![], vec![]);
    }

    let mut plus_dm = vec![0.0; high.len()];
    let mut minus_dm = vec![0.0; high.len()];

    for i in 1..high.len() {
        let up_move = high[i] - high[i - 1];
        let down_move = low[i - 1] - low[i];

        if up_move > down_move && up_move > 0.0 {
            plus_dm[i] = up_move;
        }
        if down_move > up_move && down_move > 0.0 {
            minus_dm[i] = down_move;
        }
    }

    // Use Wilder's smoothing for DM and ATR
    let smoothed_plus_dm = wilders_smooth(&plus_dm, period);
    let smoothed_minus_dm = wilders_smooth(&minus_dm, period);
    let atr_values = atr(high, low, close, period);

    // Calculate DI as smoothed_DM / ATR * 100
    let mut plus_di = Vec::with_capacity(high.len());
    let mut minus_di = Vec::with_capacity(high.len());

    for i in 0..high.len() {
        match (
            smoothed_plus_dm.get(i),
            smoothed_minus_dm.get(i),
            atr_values.get(i),
        ) {
            (Some(Some(pdm)), Some(Some(mdm)), Some(Some(atr_val))) if *atr_val > 0.0 => {
                plus_di.push(Some(pdm / atr_val * 100.0));
                minus_di.push(Some(mdm / atr_val * 100.0));
            }
            _ => {
                plus_di.push(None);
                minus_di.push(None);
            }
        }
    }

    (plus_di, minus_di)
}

/// Calculate Average Directional Index (ADX) using Wilder's smoothing
///
/// ADX requires 2*period - 1 warmup bars:
/// - First period for DI values to become valid
/// - Second period for ADX smoothing of DX values
pub fn adx(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    if high.is_empty() || period == 0 {
        return vec![];
    }

    let (plus_di, minus_di) = dmi(high, low, close, period);

    // DI values become valid at index (period - 1)
    let di_start = period - 1;

    let mut result = vec![None; high.len()];

    // Calculate DX only where DI is valid
    let mut dx_values: Vec<f64> = Vec::new();
    for i in di_start..high.len() {
        if let (Some(pdi), Some(mdi)) = (
            plus_di.get(i).and_then(|x| *x),
            minus_di.get(i).and_then(|x| *x),
        ) {
            let sum = pdi + mdi;
            if sum > 0.0 {
                dx_values.push((pdi - mdi).abs() / sum * 100.0);
            } else {
                dx_values.push(0.0);
            }
        } else {
            dx_values.push(0.0);
        }
    }

    // Apply Wilder's smoothing to the DX values
    // ADX first valid at index (period - 1) of DX, which is bar (di_start + period - 1) = (2*period - 2)
    if dx_values.len() >= period {
        let mut adx_value: Option<f64> = None;

        for (j, &dx) in dx_values.iter().enumerate() {
            let bar_idx = di_start + j;

            if j + 1 < period {
                // Not enough DX values yet
            } else if j + 1 == period {
                // First ADX value is SMA of first `period` DX values
                let sum: f64 = dx_values[0..period].iter().sum();
                adx_value = Some(sum / period as f64);
                result[bar_idx] = adx_value;
            } else {
                // Wilder's smoothing for subsequent values
                if let Some(prev_adx) = adx_value {
                    let new_adx = (prev_adx * (period - 1) as f64 + dx) / period as f64;
                    adx_value = Some(new_adx);
                    result[bar_idx] = adx_value;
                }
            }
        }
    }

    result
}

// =============================================================================
// Volume Indicators
// =============================================================================

/// Calculate Volume Weighted Average Price (VWAP)
/// Note: VWAP typically resets daily, this is cumulative for the given data
pub fn vwap(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<f64> {
    let mut cumulative_tp_vol = 0.0;
    let mut cumulative_vol = 0.0;
    let mut result = Vec::with_capacity(close.len());

    for i in 0..close.len() {
        let typical_price = (high[i] + low[i] + close[i]) / 3.0;
        cumulative_tp_vol += typical_price * volume[i];
        cumulative_vol += volume[i];

        if cumulative_vol > 0.0 {
            result.push(cumulative_tp_vol / cumulative_vol);
        } else {
            result.push(close[i]);
        }
    }

    result
}

/// Calculate On-Balance Volume (OBV) using ta crate
pub fn obv(close: &[f64], volume: &[f64]) -> Vec<f64> {
    if close.is_empty() || close.len() != volume.len() {
        return vec![];
    }

    let mut indicator = OnBalanceVolume::new();
    let mut result = Vec::with_capacity(close.len());

    for i in 0..close.len() {
        let item = make_data_item(close[i], close[i], close[i], close[i], volume[i]);
        let obv_val = indicator.next(&item);
        result.push(obv_val);
    }

    result
}

/// Calculate Money Flow Index (MFI) using ta crate
pub fn mfi(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    period: usize,
) -> Vec<Option<f64>> {
    if high.is_empty()
        || high.len() != low.len()
        || high.len() != close.len()
        || high.len() != volume.len()
    {
        return vec![];
    }

    let mut indicator = match TaMFI::new(period) {
        Ok(i) => i,
        Err(_) => return vec![None; high.len()],
    };

    let mut result = Vec::with_capacity(high.len());

    for i in 0..high.len() {
        let item = make_data_item(close[i], high[i], low[i], close[i], volume[i]);
        let mfi_val = indicator.next(&item);
        if i + 1 >= period {
            result.push(Some(mfi_val));
        } else {
            result.push(None);
        }
    }

    result
}

// =============================================================================
// Indicator Cache for Optimization
// =============================================================================

/// Cached indicator calculator for performance optimization
/// Cache keys are (data_length, period) tuples to identify unique calculations
pub struct IndicatorCache {
    sma_cache: HashMap<(usize, usize), Vec<Option<f64>>>,
    ema_cache: HashMap<(usize, usize), Vec<Option<f64>>>,
    atr_cache: HashMap<(usize, usize), Vec<Option<f64>>>,
    rsi_cache: HashMap<(usize, usize), Vec<Option<f64>>>,
}

impl IndicatorCache {
    pub fn new() -> Self {
        IndicatorCache {
            sma_cache: HashMap::new(),
            ema_cache: HashMap::new(),
            atr_cache: HashMap::new(),
            rsi_cache: HashMap::new(),
        }
    }

    pub fn get_sma(&mut self, values: &[f64], period: usize) -> &Vec<Option<f64>> {
        let key = (values.len(), period);
        self.sma_cache
            .entry(key)
            .or_insert_with(|| sma(values, period))
    }

    pub fn get_ema(&mut self, values: &[f64], period: usize) -> &Vec<Option<f64>> {
        let key = (values.len(), period);
        self.ema_cache
            .entry(key)
            .or_insert_with(|| ema(values, period))
    }

    pub fn get_atr(
        &mut self,
        high: &[f64],
        low: &[f64],
        close: &[f64],
        period: usize,
    ) -> &Vec<Option<f64>> {
        let key = (close.len(), period);
        self.atr_cache
            .entry(key)
            .or_insert_with(|| atr(high, low, close, period))
    }

    pub fn get_rsi(&mut self, values: &[f64], period: usize) -> &Vec<Option<f64>> {
        let key = (values.len(), period);
        self.rsi_cache
            .entry(key)
            .or_insert_with(|| rsi(values, period))
    }

    pub fn clear(&mut self) {
        self.sma_cache.clear();
        self.ema_cache.clear();
        self.atr_cache.clear();
        self.rsi_cache.clear();
    }
}

impl Default for IndicatorCache {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Incremental Indicators
// =============================================================================

/// Incremental ADX Calculator
/// Uses Wilder's Smoothing for all components
#[derive(Debug, Clone)]
pub struct IncrementalAdx {
    period: usize,
    prev_high: f64,
    prev_low: f64,
    prev_close: f64,
    
    // Smoothed values (Wilder's)
    smooth_tr: f64,
    smooth_pos_dm: f64,
    smooth_neg_dm: f64,
    smooth_dx: f64, // This is ADX
    
    count: usize,
}

impl IncrementalAdx {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prev_high: 0.0,
            prev_low: 0.0,
            prev_close: 0.0,
            smooth_tr: 0.0,
            smooth_pos_dm: 0.0,
            smooth_neg_dm: 0.0,
            smooth_dx: 0.0,
            count: 0,
        }
    }

    pub fn next(&mut self, high: f64, low: f64, close: f64) -> f64 {
        if self.count == 0 {
            self.prev_high = high;
            self.prev_low = low;
            self.prev_close = close;
            self.count += 1;
            return 0.0;
        }

        // 1. Calculate TR
        let hl = high - low;
        let hc = (high - self.prev_close).abs();
        let lc = (low - self.prev_close).abs();
        let tr = hl.max(hc).max(lc);

        // 2. Calculate Directional Movement
        let up = high - self.prev_high;
        let down = self.prev_low - low;
        
        let pos_dm = if up > down && up > 0.0 { up } else { 0.0 };
        let neg_dm = if down > up && down > 0.0 { down } else { 0.0 };

        // 3. Update Smoothed Values (Wilder's)
        
        if self.count <= self.period {
            // Initial SMA phase for Wilder's
            self.smooth_tr += tr;
            self.smooth_pos_dm += pos_dm;
            self.smooth_neg_dm += neg_dm;
            
            if self.count == self.period {
                // Finalize initial average
                // Note: The standard definition effectively sums then divides, 
                // but for incremental we need to transition to smoothing.
                // However, Wilder's usually starts with SMA.
                // Let's keep sums until period is reached, then divide?
                // No, typically you accumulate, then at period, you have the sum.
                // But let's just use the sum as the seed for the next step?
                // No, the formula is: New = Prev + (Curr - Prev)/N
                // Or New = (Prev*(N-1) + Curr)/N
                
                // Let's assume at count==period, we convert sum to average
                // but wait, next step needs the previous average.
                // So at step period, we store average.
            }
        } else {
            // Wilder's Smoothing: Val[i] = (Val[i-1] * (n-1) + Curr) / n
            self.smooth_tr = (self.smooth_tr * (self.period - 1) as f64 + tr) / self.period as f64;
            self.smooth_pos_dm = (self.smooth_pos_dm * (self.period - 1) as f64 + pos_dm) / self.period as f64;
            self.smooth_neg_dm = (self.smooth_neg_dm * (self.period - 1) as f64 + neg_dm) / self.period as f64;
        }

        // Handle the transition at self.count == self.period strictly
        if self.count == self.period {
             self.smooth_tr /= self.period as f64;
             self.smooth_pos_dm /= self.period as f64;
             self.smooth_neg_dm /= self.period as f64;
        }

        // 4. Calculate DI and DX
        let mut dx = 0.0;
        if self.smooth_tr > 0.0 {
            let pos_di = (self.smooth_pos_dm / self.smooth_tr) * 100.0;
            let neg_di = (self.smooth_neg_dm / self.smooth_tr) * 100.0;
            let sum_di = pos_di + neg_di;
            if sum_di > 0.0 {
                dx = (pos_di - neg_di).abs() / sum_di * 100.0;
            }
        }

        // 5. Smooth DX to get ADX
        // ADX needs 2*period - 1 to start valid
        if self.count <= 2 * self.period {
             self.smooth_dx += dx;
             if self.count == 2 * self.period {
                 self.smooth_dx /= self.period as f64;
             }
        } else {
             self.smooth_dx = (self.smooth_dx * (self.period - 1) as f64 + dx) / self.period as f64;
        }

        // Update state
        self.prev_high = high;
        self.prev_low = low;
        self.prev_close = close;
        self.count += 1;

        if self.count > 2 * self.period {
            self.smooth_dx
        } else {
            0.0 // Not ready
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sma() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = sma(&values, 3);

        assert_eq!(result[0], None);
        assert_eq!(result[1], None);
        assert!(result[2].is_some());
        // SMA of [1,2,3] = 2.0
        assert!((result[2].unwrap() - 2.0).abs() < 0.001);
        // SMA of [2,3,4] = 3.0
        assert!((result[3].unwrap() - 3.0).abs() < 0.001);
        // SMA of [3,4,5] = 4.0
        assert!((result[4].unwrap() - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_ema() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = ema(&values, 3);

        assert_eq!(result[0], None);
        assert_eq!(result[1], None);
        assert!(result[2].is_some());
        // EMA values should be reasonable
        let ema_val = result[4].unwrap();
        assert!(ema_val > 3.0 && ema_val < 5.0);
    }

    #[test]
    fn test_rsi() {
        let values = vec![
            44.0, 44.25, 44.5, 43.75, 44.5, 44.25, 44.0, 43.5, 44.0, 44.5, 45.0, 45.25, 45.5, 45.0,
            44.75,
        ];
        let result = rsi(&values, 14);

        // RSI should have values after period warmup
        assert!(result.last().unwrap().is_some());
        let rsi_val = result.last().unwrap().unwrap();
        // RSI should be between 0 and 100
        assert!((0.0..=100.0).contains(&rsi_val));
    }

    #[test]
    fn test_macd() {
        let values: Vec<f64> = (1..=50).map(|x| x as f64).collect();
        let (macd_line, signal, histogram) = macd(&values, 12, 26, 9);

        // MACD should have values after slow period warmup
        assert!(macd_line.last().unwrap().is_some());
        assert!(signal.last().unwrap().is_some());
        assert!(histogram.last().unwrap().is_some());
    }

    #[test]
    fn test_stochastic() {
        let high = vec![5.0, 6.0, 7.0, 8.0, 9.0, 8.0, 7.0, 8.0, 9.0, 10.0];
        let low = vec![4.0, 5.0, 6.0, 7.0, 8.0, 7.0, 6.0, 7.0, 8.0, 9.0];
        let close = vec![4.5, 5.5, 6.5, 7.5, 8.5, 7.5, 6.5, 7.5, 8.5, 9.5];

        let (k, d) = stochastic(&high, &low, &close, 5, 3);

        // After warmup, we should have values
        let last_k = k.last().unwrap();
        let last_d = d.last().unwrap();
        assert!(last_k.is_some());
        assert!(last_d.is_some());

        // %K and %D should be between 0 and 100
        let k_val = last_k.unwrap();
        assert!(
            (0.0..=100.0).contains(&k_val),
            "Stochastic %K should be between 0 and 100, got {}",
            k_val
        );
    }

    #[test]
    fn test_bollinger_bands() {
        let values = vec![20.0, 21.0, 22.0, 21.0, 20.0, 21.0, 22.0, 23.0, 22.0, 21.0];
        let (upper, middle, lower) = bollinger_bands(&values, 5, 2.0);

        assert!(upper[4].is_some());
        assert!(middle[4].is_some());
        assert!(lower[4].is_some());

        // Upper > Middle > Lower
        assert!(upper[4].unwrap() > middle[4].unwrap());
        assert!(middle[4].unwrap() > lower[4].unwrap());
    }

    #[test]
    fn test_atr() {
        let high = vec![10.0, 11.0, 12.0, 11.5, 12.0];
        let low = vec![9.0, 10.0, 11.0, 10.5, 11.0];
        let close = vec![9.5, 10.5, 11.5, 11.0, 11.5];

        let result = atr(&high, &low, &close, 3);

        assert!(result[2].is_some());
        let atr_val = result[2].unwrap();
        assert!(atr_val > 0.0);
    }

    #[test]
    fn test_vwap() {
        let high = vec![10.0, 11.0, 12.0, 11.0, 10.0];
        let low = vec![9.0, 10.0, 11.0, 10.0, 9.0];
        let close = vec![9.5, 10.5, 11.5, 10.5, 9.5];
        let volume = vec![100.0, 150.0, 200.0, 150.0, 100.0];

        let result = vwap(&high, &low, &close, &volume);

        assert_eq!(result.len(), 5);
        // VWAP should be somewhere around the average price
        assert!(result[4] > 9.0 && result[4] < 12.0);
    }

    #[test]
    fn test_obv() {
        let close = vec![10.0, 11.0, 10.5, 11.5, 11.0];
        let volume = vec![100.0, 150.0, 120.0, 180.0, 90.0];

        let result = obv(&close, &volume);

        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_cci() {
        let high = vec![10.0, 11.0, 12.0, 11.5, 12.0, 13.0, 12.5, 13.0, 14.0, 13.5];
        let low = vec![9.0, 10.0, 11.0, 10.5, 11.0, 12.0, 11.5, 12.0, 13.0, 12.5];
        let close = vec![9.5, 10.5, 11.5, 11.0, 11.5, 12.5, 12.0, 12.5, 13.5, 13.0];

        let result = cci(&high, &low, &close, 5);

        assert!(result[4].is_some());
    }

    #[test]
    fn test_indicator_cache() {
        let mut cache = IndicatorCache::new();
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        // First call calculates
        let sma1 = cache.get_sma(&values, 3).clone();
        // Second call uses cache
        let sma2 = cache.get_sma(&values, 3).clone();

        assert_eq!(sma1, sma2);
    }

    #[test]
    fn test_keltner_channels() {
        let high = vec![10.0, 11.0, 12.0, 11.5, 12.0, 13.0, 12.5, 13.0, 14.0, 13.5];
        let low = vec![9.0, 10.0, 11.0, 10.5, 11.0, 12.0, 11.5, 12.0, 13.0, 12.5];
        let close = vec![9.5, 10.5, 11.5, 11.0, 11.5, 12.5, 12.0, 12.5, 13.5, 13.0];

        let (upper, middle, lower) = keltner_channels(&high, &low, &close, 5, 5, 2.0);

        // After warmup
        let last_idx = close.len() - 1;
        assert!(upper[last_idx].is_some());
        assert!(middle[last_idx].is_some());
        assert!(lower[last_idx].is_some());

        // Upper > Middle > Lower
        assert!(upper[last_idx].unwrap() > middle[last_idx].unwrap());
        assert!(middle[last_idx].unwrap() > lower[last_idx].unwrap());
    }
}
