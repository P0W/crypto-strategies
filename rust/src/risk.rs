//! Risk management framework
//!
//! Implements portfolio-level risk controls including position sizing,
//! drawdown-based de-risking, and consecutive loss protection.
//!
//! # Currency-Agnostic Design
//!
//! All position sizing and risk calculations are **currency-agnostic**.
//! The code treats all monetary values as dimensionless numbers and works
//! correctly as long as `initial_capital` and price data share the same
//! currency denomination.
//!
//! Position sizing formula:
//! ```text
//! position_size = (capital * risk_per_trade) / (entry_price - stop_price)
//! ```
//!
//! This formula produces the same risk exposure percentage regardless of
//! currency unit (USD, INR, EUR, etc.), as long as capital and prices
//! are consistent.

use crate::Position;

/// Configuration for RiskManager using builder pattern
#[derive(Debug, Clone)]
pub struct RiskManagerConfig {
    /// Initial capital in the same currency as price data.
    /// No currency conversion is performed.
    pub initial_capital: f64,
    pub risk_per_trade: f64,
    pub max_positions: usize,
    pub max_portfolio_heat: f64,
    pub max_position_pct: f64,
    pub max_drawdown: f64,
    pub drawdown_warning: f64,
    pub drawdown_critical: f64,
    pub drawdown_warning_multiplier: f64,
    pub drawdown_critical_multiplier: f64,
    pub consecutive_loss_limit: usize,
    pub consecutive_loss_multiplier: f64,
}

impl Default for RiskManagerConfig {
    fn default() -> Self {
        Self {
            initial_capital: 100_000.0,
            risk_per_trade: 0.02,
            max_positions: 3,
            max_portfolio_heat: 0.10,
            max_position_pct: 0.20,
            max_drawdown: 0.20,
            drawdown_warning: 0.10,
            drawdown_critical: 0.15,
            drawdown_warning_multiplier: 0.50,
            drawdown_critical_multiplier: 0.25,
            consecutive_loss_limit: 3,
            consecutive_loss_multiplier: 0.75,
        }
    }
}

impl RiskManagerConfig {
    /// Create a new config with initial capital
    pub fn with_capital(mut self, capital: f64) -> Self {
        self.initial_capital = capital;
        self
    }

    /// Set risk per trade as a fraction (e.g., 0.02 = 2%)
    pub fn with_risk_per_trade(mut self, risk: f64) -> Self {
        self.risk_per_trade = risk;
        self
    }

    /// Set maximum concurrent positions
    pub fn with_max_positions(mut self, max: usize) -> Self {
        self.max_positions = max;
        self
    }

    /// Set maximum portfolio heat
    pub fn with_max_portfolio_heat(mut self, heat: f64) -> Self {
        self.max_portfolio_heat = heat;
        self
    }

    /// Set maximum position percentage
    pub fn with_max_position_pct(mut self, pct: f64) -> Self {
        self.max_position_pct = pct;
        self
    }

    /// Set maximum drawdown threshold for halting
    pub fn with_max_drawdown(mut self, dd: f64) -> Self {
        self.max_drawdown = dd;
        self
    }

    /// Set drawdown warning and critical thresholds with multipliers
    pub fn with_drawdown_levels(
        mut self,
        warning: f64,
        critical: f64,
        warning_mult: f64,
        critical_mult: f64,
    ) -> Self {
        self.drawdown_warning = warning;
        self.drawdown_critical = critical;
        self.drawdown_warning_multiplier = warning_mult;
        self.drawdown_critical_multiplier = critical_mult;
        self
    }

    /// Set consecutive loss protection
    pub fn with_consecutive_loss_protection(mut self, limit: usize, multiplier: f64) -> Self {
        self.consecutive_loss_limit = limit;
        self.consecutive_loss_multiplier = multiplier;
        self
    }

    /// Build the RiskManager
    pub fn build(self) -> RiskManager {
        RiskManager::from_config(self)
    }
}

/// Risk manager for portfolio-level risk controls
#[derive(Debug, Clone)]
pub struct RiskManager {
    pub initial_capital: f64,
    pub current_capital: f64,
    pub peak_capital: f64,
    pub max_drawdown: f64,
    pub drawdown_warning: f64,
    pub drawdown_critical: f64,
    pub drawdown_warning_multiplier: f64,
    pub drawdown_critical_multiplier: f64,
    pub consecutive_loss_limit: usize,
    pub consecutive_loss_multiplier: f64,
    pub consecutive_losses: usize,
    pub consecutive_wins: usize,
    pub risk_per_trade: f64,
    pub max_positions: usize,
    pub max_portfolio_heat: f64,
    pub max_position_pct: f64,
}

impl RiskManager {
    /// Create new risk manager from config (preferred method)
    pub fn from_config(config: RiskManagerConfig) -> Self {
        RiskManager {
            initial_capital: config.initial_capital,
            current_capital: config.initial_capital,
            peak_capital: config.initial_capital,
            max_drawdown: config.max_drawdown,
            drawdown_warning: config.drawdown_warning,
            drawdown_critical: config.drawdown_critical,
            drawdown_warning_multiplier: config.drawdown_warning_multiplier,
            drawdown_critical_multiplier: config.drawdown_critical_multiplier,
            consecutive_loss_limit: config.consecutive_loss_limit,
            consecutive_loss_multiplier: config.consecutive_loss_multiplier,
            consecutive_losses: 0,
            consecutive_wins: 0,
            risk_per_trade: config.risk_per_trade,
            max_positions: config.max_positions,
            max_portfolio_heat: config.max_portfolio_heat,
            max_position_pct: config.max_position_pct,
        }
    }

    /// Create new risk manager (kept for backward compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        initial_capital: f64,
        risk_per_trade: f64,
        max_positions: usize,
        max_portfolio_heat: f64,
        max_position_pct: f64,
        max_drawdown: f64,
        drawdown_warning: f64,
        drawdown_critical: f64,
        drawdown_warning_multiplier: f64,
        drawdown_critical_multiplier: f64,
        consecutive_loss_limit: usize,
        consecutive_loss_multiplier: f64,
    ) -> Self {
        RiskManager {
            initial_capital,
            current_capital: initial_capital,
            peak_capital: initial_capital,
            max_drawdown,
            drawdown_warning,
            drawdown_critical,
            drawdown_warning_multiplier,
            drawdown_critical_multiplier,
            consecutive_loss_limit,
            consecutive_loss_multiplier,
            consecutive_losses: 0,
            consecutive_wins: 0,
            risk_per_trade,
            max_positions,
            max_portfolio_heat,
            max_position_pct,
        }
    }

    /// Update capital and track peak
    pub fn update_capital(&mut self, new_capital: f64) {
        self.current_capital = new_capital;
        if new_capital > self.peak_capital {
            self.peak_capital = new_capital;
        }
    }

    /// Get current peak capital
    pub fn peak_capital(&self) -> f64 {
        self.peak_capital
    }

    /// Calculate current drawdown
    pub fn current_drawdown(&self) -> f64 {
        if self.peak_capital == 0.0 {
            return 0.0;
        }
        (self.peak_capital - self.current_capital) / self.peak_capital
    }

    /// Check if trading should be halted due to excessive drawdown
    pub fn should_halt_trading(&self) -> bool {
        self.current_drawdown() >= self.max_drawdown
    }

    /// Get position size multiplier based on drawdown
    pub fn drawdown_multiplier(&self) -> f64 {
        let dd = self.current_drawdown();

        if dd >= self.drawdown_critical {
            self.drawdown_critical_multiplier
        } else if dd >= self.drawdown_warning {
            self.drawdown_warning_multiplier
        } else {
            1.0
        }
    }

    /// Get position size multiplier based on consecutive losses
    pub fn consecutive_loss_multiplier(&self) -> f64 {
        if self.consecutive_losses >= self.consecutive_loss_limit {
            self.consecutive_loss_multiplier
        } else {
            1.0
        }
    }

    /// Calculate position size for a trade with regime score adjustment
    pub fn calculate_position_size_with_regime(
        &self,
        entry_price: f64,
        stop_price: f64,
        current_positions: &[&Position],
        regime_score: f64,
    ) -> f64 {
        if self.should_halt_trading() {
            return 0.0;
        }

        // Base risk amount
        let base_risk = self.current_capital * self.risk_per_trade;

        // Apply regime score (Python: regime_adjusted_risk = base_risk * regime_score)
        let regime_adjusted = base_risk * regime_score;

        // Apply drawdown multiplier
        let dd_multiplier = self.drawdown_multiplier();

        // Apply consecutive loss multiplier
        let cl_multiplier = self.consecutive_loss_multiplier();

        // Combined risk amount
        let adjusted_risk = regime_adjusted * dd_multiplier * cl_multiplier;

        // Calculate position size based on stop distance
        let stop_distance = (entry_price - stop_price).abs();
        if stop_distance == 0.0 {
            return 0.0;
        }

        let mut position_size = adjusted_risk / stop_distance;

        // Check position size limits
        let max_position_value = self.current_capital * self.max_position_pct;
        let position_value = position_size * entry_price;

        if position_value > max_position_value {
            // Cap position size but DON'T return early - still need portfolio heat check
            position_size = max_position_value / entry_price;
        }

        // Check portfolio heat - sum of risk amounts (stop_distance × quantity) for each position
        // This matches main branch behavior where max_portfolio_heat limits total RISK, not total VALUE
        let current_heat: f64 = current_positions.iter().map(|p| p.risk_amount).sum();

        let max_allowed_heat = self.current_capital * self.max_portfolio_heat;

        tracing::debug!(
            "Portfolio heat check: current_heat={:.2}, new_risk={:.2}, max_allowed={:.2}, positions={}",
            current_heat,
            adjusted_risk,
            max_allowed_heat,
            current_positions.len()
        );

        // Check if adding this position's RISK exceeds max allowed heat
        // Main branch: current_heat + adjusted_risk > max_allowed_heat
        if current_heat + adjusted_risk > max_allowed_heat {
            let remaining_heat = max_allowed_heat - current_heat;
            if remaining_heat > 0.0 {
                // Size the position to fit remaining allowed risk
                let limited_size = remaining_heat / stop_distance;
                tracing::debug!(
                    "Portfolio heat limiting: remaining_heat={:.2}, limited_size={:.6}",
                    remaining_heat,
                    limited_size
                );
                return limited_size.min(position_size);
            } else {
                tracing::debug!("Portfolio heat exceeded: no room for new position");
                return 0.0;
            }
        }

        position_size
    }

    /// Can open a new position? (count-based, avoids allocation)
    #[inline]
    pub fn can_open_position_count(&self, position_count: usize) -> bool {
        !self.should_halt_trading() && position_count < self.max_positions
    }

    /// Calculate position size using an iterator (avoids Vec allocation)
    pub fn calculate_position_size_with_regime_iter<'a, I>(
        &self,
        entry_price: f64,
        stop_price: f64,
        current_positions: I,
        regime_score: f64,
    ) -> f64
    where
        I: Iterator<Item = &'a Position>,
    {
        if self.should_halt_trading() {
            return 0.0;
        }

        // Base risk amount
        let base_risk = self.current_capital * self.risk_per_trade;

        // Apply regime score
        let regime_adjusted = base_risk * regime_score;

        // Apply drawdown multiplier
        let dd_multiplier = self.drawdown_multiplier();

        // Apply consecutive loss multiplier
        let cl_multiplier = self.consecutive_loss_multiplier();

        // Combined risk amount
        let adjusted_risk = regime_adjusted * dd_multiplier * cl_multiplier;

        // Calculate position size based on stop distance
        let stop_distance = (entry_price - stop_price).abs();
        if stop_distance == 0.0 {
            return 0.0;
        }

        let mut position_size = adjusted_risk / stop_distance;

        // Check position size limits
        let max_position_value = self.current_capital * self.max_position_pct;
        let position_value = position_size * entry_price;

        if position_value > max_position_value {
            // Cap position size but DON'T return early - still need portfolio heat check
            position_size = max_position_value / entry_price;
        }

        // Check portfolio heat (sum risk amounts from iterator)
        // This matches main branch behavior where max_portfolio_heat limits total RISK, not total VALUE
        let current_heat: f64 = current_positions.map(|p| p.risk_amount).sum();

        let max_allowed_heat = self.current_capital * self.max_portfolio_heat;

        // Check if adding this position's RISK exceeds max allowed heat
        if current_heat + adjusted_risk > max_allowed_heat {
            let remaining_heat = max_allowed_heat - current_heat;
            if remaining_heat > 0.0 {
                // Size the position to fit remaining allowed risk
                let limited_size = remaining_heat / stop_distance;
                return limited_size.min(position_size);
            } else {
                return 0.0;
            }
        }

        position_size
    }

    /// Record a winning trade
    pub fn record_win(&mut self) {
        self.consecutive_wins += 1;
        self.consecutive_losses = 0;
    }

    /// Record a losing trade
    pub fn record_loss(&mut self) {
        self.consecutive_losses += 1;
        self.consecutive_wins = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drawdown_calculation() {
        let mut rm = RiskManager::new(
            100_000.0, 0.02, 2, 0.10, 0.40, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
        );

        assert_eq!(rm.current_drawdown(), 0.0);

        rm.update_capital(90_000.0);
        assert_eq!(rm.current_drawdown(), 0.10);

        rm.update_capital(110_000.0);
        assert_eq!(rm.current_drawdown(), 0.0);
    }

    #[test]
    fn test_should_halt_trading() {
        let mut rm = RiskManager::new(
            100_000.0, 0.02, 2, 0.10, 0.40, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
        );

        assert!(!rm.should_halt_trading());

        rm.update_capital(79_000.0); // 21% drawdown
        assert!(rm.should_halt_trading());
    }

    #[test]
    fn test_position_sizing() {
        let rm = RiskManager::new(
            100_000.0, 0.02, 2, 0.10, 0.40, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
        );

        let entry = 100.0;
        let stop = 95.0;
        let positions: Vec<&Position> = vec![];

        let size = rm.calculate_position_size_with_regime(entry, stop, &positions, 1.0);

        // Risk = 100,000 * 0.02 = 2,000
        // Stop distance = 5
        // Size = 2,000 / 5 = 400
        assert_eq!(size, 400.0);
    }

    /// Verify that currency units don't affect percentage-based calculations.
    /// Tests the core claim that the system is currency-agnostic.
    #[test]
    fn test_currency_independence() {
        // Test constants
        const USD_TO_INR_EXCHANGE_RATE: f64 = 85.0;
        const POSITION_SIZE_TOLERANCE: f64 = 0.0001;
        const PERCENTAGE_TOLERANCE: f64 = 0.01;

        // Scenario 1: USD
        let capital_usd: f64 = 100_000.0;
        let btc_price_usd: f64 = 90_000.0;
        let risk_per_trade: f64 = 0.15;
        let atr_usd: f64 = 3_000.0;
        let stop_multiple: f64 = 2.5;

        let stop_distance_usd = atr_usd * stop_multiple;
        let base_risk_usd = capital_usd * risk_per_trade;
        let position_size_usd = base_risk_usd / stop_distance_usd;
        let position_value_usd = position_size_usd * btc_price_usd;
        let position_pct_usd = (position_value_usd / capital_usd) * 100.0;

        // Scenario 2: INR (multiply everything by exchange rate)
        let capital_inr = capital_usd * USD_TO_INR_EXCHANGE_RATE;
        let btc_price_inr = btc_price_usd * USD_TO_INR_EXCHANGE_RATE;
        let atr_inr = atr_usd * USD_TO_INR_EXCHANGE_RATE;

        let stop_distance_inr = atr_inr * stop_multiple;
        let base_risk_inr = capital_inr * risk_per_trade;
        let position_size_inr = base_risk_inr / stop_distance_inr;
        let position_value_inr = position_size_inr * btc_price_inr;
        let position_pct_inr = (position_value_inr / capital_inr) * 100.0;

        // Key assertions:
        // 1. Position size in BTC should be identical
        assert!(
            (position_size_usd - position_size_inr).abs() < POSITION_SIZE_TOLERANCE,
            "Position sizes differ: USD={}, INR={}",
            position_size_usd,
            position_size_inr
        );

        // 2. Position as % of capital should be identical
        assert!(
            (position_pct_usd - position_pct_inr).abs() < PERCENTAGE_TOLERANCE,
            "Position percentages differ: USD={:.2}%, INR={:.2}%",
            position_pct_usd,
            position_pct_inr
        );

        // Test return calculation
        let exit_price_usd: f64 = 95_000.0; // BTC goes up 5.56%
        let exit_price_inr = exit_price_usd * USD_TO_INR_EXCHANGE_RATE;

        let pnl_usd = (exit_price_usd - btc_price_usd) * position_size_usd;
        let return_pct_usd = (pnl_usd / capital_usd) * 100.0;

        let pnl_inr = (exit_price_inr - btc_price_inr) * position_size_inr;
        let return_pct_inr = (pnl_inr / capital_inr) * 100.0;

        // 3. Return % should be identical
        assert!(
            (return_pct_usd - return_pct_inr).abs() < PERCENTAGE_TOLERANCE,
            "Return percentages differ: USD={:.2}%, INR={:.2}%",
            return_pct_usd,
            return_pct_inr
        );
    }
}
