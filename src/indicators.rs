//! Technical indicators
//!
//! Implementation of common technical indicators used in trading strategies.

/// Calculate Simple Moving Average
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut result = Vec::with_capacity(values.len());

    for i in 0..values.len() {
        if i + 1 < period {
            result.push(None);
        } else {
            let sum: f64 = values[i + 1 - period..=i].iter().sum();
            result.push(Some(sum / period as f64));
        }
    }

    result
}

/// Calculate Exponential Moving Average
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut result = Vec::with_capacity(values.len());
    
    if values.is_empty() || period == 0 {
        return result;
    }

    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut ema_value: Option<f64> = None;

    for (i, &value) in values.iter().enumerate() {
        if i < period - 1 {
            result.push(None);
        } else if i == period - 1 {
            // Initialize with SMA
            let sum: f64 = values[0..period].iter().sum();
            ema_value = Some(sum / period as f64);
            result.push(ema_value);
        } else {
            if let Some(prev_ema) = ema_value {
                let new_ema = (value - prev_ema) * multiplier + prev_ema;
                ema_value = Some(new_ema);
                result.push(Some(new_ema));
            }
        }
    }

    result
}

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

/// Calculate Average True Range (ATR)
pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let tr = true_range(high, low, close);
    ema(&tr, period)
}

/// Calculate Directional Movement Index (DMI) components
pub fn dmi(high: &[f64], low: &[f64], _close: &[f64], period: usize) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
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

    let plus_di = ema(&plus_dm, period);
    let minus_di = ema(&minus_dm, period);

    (plus_di, minus_di)
}

/// Calculate Average Directional Index (ADX)
pub fn adx(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let (plus_di, minus_di) = dmi(high, low, close, period);
    let atr_values = atr(high, low, close, period);

    let mut dx = Vec::with_capacity(high.len());

    for i in 0..high.len() {
        if let (Some(pdi), Some(mdi), Some(atr_val)) = (plus_di[i], minus_di[i], atr_values[i]) {
            if atr_val > 0.0 {
                let pdi_norm = pdi / atr_val * 100.0;
                let mdi_norm = mdi / atr_val * 100.0;
                
                let sum = pdi_norm + mdi_norm;
                if sum > 0.0 {
                    let dx_val = ((pdi_norm - mdi_norm).abs() / sum) * 100.0;
                    dx.push(dx_val);
                } else {
                    dx.push(0.0);
                }
            } else {
                dx.push(0.0);
            }
        } else {
            dx.push(0.0);
        }
    }

    ema(&dx, period)
}

/// Calculate Bollinger Bands
pub fn bollinger_bands(
    values: &[f64],
    period: usize,
    num_std: f64,
) -> (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>) {
    let middle = sma(values, period);
    let mut upper = Vec::with_capacity(values.len());
    let mut lower = Vec::with_capacity(values.len());

    for i in 0..values.len() {
        if let Some(mid) = middle[i] {
            if i + 1 >= period {
                let window = &values[i + 1 - period..=i];
                let variance: f64 = window
                    .iter()
                    .map(|&x| {
                        let diff = x - mid;
                        diff * diff
                    })
                    .sum::<f64>()
                    / period as f64;
                let std_dev = variance.sqrt();

                upper.push(Some(mid + num_std * std_dev));
                lower.push(Some(mid - num_std * std_dev));
            } else {
                upper.push(None);
                lower.push(None);
            }
        } else {
            upper.push(None);
            lower.push(None);
        }
    }

    (upper, middle, lower)
}

/// Calculate RSI (Relative Strength Index)
pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut gains = Vec::with_capacity(values.len());
    let mut losses = Vec::with_capacity(values.len());

    gains.push(0.0);
    losses.push(0.0);

    for i in 1..values.len() {
        let change = values[i] - values[i - 1];
        gains.push(if change > 0.0 { change } else { 0.0 });
        losses.push(if change < 0.0 { -change } else { 0.0 });
    }

    let avg_gains = ema(&gains, period);
    let avg_losses = ema(&losses, period);

    let mut rsi_values = Vec::with_capacity(values.len());

    for i in 0..values.len() {
        if let (Some(avg_gain), Some(avg_loss)) = (avg_gains[i], avg_losses[i]) {
            if avg_loss == 0.0 {
                rsi_values.push(Some(100.0));
            } else {
                let rs = avg_gain / avg_loss;
                let rsi_val = 100.0 - (100.0 / (1.0 + rs));
                rsi_values.push(Some(rsi_val));
            }
        } else {
            rsi_values.push(None);
        }
    }

    rsi_values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sma() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = sma(&values, 3);
        
        assert_eq!(result[0], None);
        assert_eq!(result[1], None);
        assert_eq!(result[2], Some(2.0));
        assert_eq!(result[3], Some(3.0));
        assert_eq!(result[4], Some(4.0));
    }

    #[test]
    fn test_ema() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = ema(&values, 3);
        
        assert_eq!(result[0], None);
        assert_eq!(result[1], None);
        assert!(result[2].is_some());
    }
}
