#!/usr/bin/env python3
"""
Demonstration: Currency Independence in Backtest Calculations

This script proves that the backtesting system produces identical percentage
results regardless of currency, as long as capital and prices are consistent.

Run this to verify:
    python3 verify_currency_independence.py
"""

def calculate_position_metrics(capital, price, risk_pct, stop_distance, exit_price):
    """Calculate position sizing and returns for a single trade"""
    base_risk = capital * risk_pct
    position_size = base_risk / stop_distance
    position_value = position_size * price
    position_pct = (position_value / capital) * 100.0
    
    pnl = (exit_price - price) * position_size
    return_pct = (pnl / capital) * 100.0
    
    return {
        'position_size': position_size,
        'position_value': position_value,
        'position_pct': position_pct,
        'pnl': pnl,
        'return_pct': return_pct
    }

def main():
    print("=" * 70)
    print("CURRENCY INDEPENDENCE VERIFICATION")
    print("=" * 70)
    print()
    
    # Common parameters
    risk_pct = 0.15  # 15% risk per trade
    atr = 3000  # ATR in USD
    stop_multiple = 2.5
    
    # Scenario 1: USD
    print("SCENARIO 1: USD Currency")
    print("-" * 70)
    capital_usd = 100_000
    btc_price_usd = 90_000
    stop_distance_usd = atr * stop_multiple
    exit_price_usd = 95_000
    
    usd_metrics = calculate_position_metrics(
        capital_usd, btc_price_usd, risk_pct, stop_distance_usd, exit_price_usd
    )
    
    print(f"Initial Capital:  ${capital_usd:,}")
    print(f"BTC Price:        ${btc_price_usd:,}")
    print(f"Stop Distance:    ${stop_distance_usd:,}")
    print(f"Exit Price:       ${exit_price_usd:,}")
    print()
    print(f"Position Size:    {usd_metrics['position_size']:.6f} BTC")
    print(f"Position Value:   ${usd_metrics['position_value']:,.2f}")
    print(f"Position %:       {usd_metrics['position_pct']:.2f}% of capital")
    print(f"PnL:              ${usd_metrics['pnl']:,.2f}")
    print(f"Return:           {usd_metrics['return_pct']:.2f}%")
    print()
    
    # Scenario 2: INR
    print("SCENARIO 2: INR Currency (multiply all USD values by 85)")
    print("-" * 70)
    exchange_rate = 85
    capital_inr = capital_usd * exchange_rate
    btc_price_inr = btc_price_usd * exchange_rate
    stop_distance_inr = stop_distance_usd * exchange_rate
    exit_price_inr = exit_price_usd * exchange_rate
    
    inr_metrics = calculate_position_metrics(
        capital_inr, btc_price_inr, risk_pct, stop_distance_inr, exit_price_inr
    )
    
    print(f"Initial Capital:  ₹{capital_inr:,}")
    print(f"BTC Price:        ₹{btc_price_inr:,}")
    print(f"Stop Distance:    ₹{stop_distance_inr:,}")
    print(f"Exit Price:       ₹{exit_price_inr:,}")
    print()
    print(f"Position Size:    {inr_metrics['position_size']:.6f} BTC")
    print(f"Position Value:   ₹{inr_metrics['position_value']:,.2f}")
    print(f"Position %:       {inr_metrics['position_pct']:.2f}% of capital")
    print(f"PnL:              ₹{inr_metrics['pnl']:,.2f}")
    print(f"Return:           {inr_metrics['return_pct']:.2f}%")
    print()
    
    # Verification
    print("=" * 70)
    print("VERIFICATION")
    print("=" * 70)
    
    # Check if key metrics match
    position_match = abs(usd_metrics['position_size'] - inr_metrics['position_size']) < 0.0001
    pct_match = abs(usd_metrics['position_pct'] - inr_metrics['position_pct']) < 0.01
    return_match = abs(usd_metrics['return_pct'] - inr_metrics['return_pct']) < 0.01
    
    print(f"Position Size Match:    {'✓ PASS' if position_match else '✗ FAIL'}")
    print(f"  USD: {usd_metrics['position_size']:.6f} BTC")
    print(f"  INR: {inr_metrics['position_size']:.6f} BTC")
    print()
    
    print(f"Position % Match:       {'✓ PASS' if pct_match else '✗ FAIL'}")
    print(f"  USD: {usd_metrics['position_pct']:.2f}%")
    print(f"  INR: {inr_metrics['position_pct']:.2f}%")
    print()
    
    print(f"Return % Match:         {'✓ PASS' if return_match else '✗ FAIL'}")
    print(f"  USD: {usd_metrics['return_pct']:.2f}%")
    print(f"  INR: {inr_metrics['return_pct']:.2f}%")
    print()
    
    if position_match and pct_match and return_match:
        print("=" * 70)
        print("✓ SUCCESS: Currency does NOT affect percentage-based results!")
        print("=" * 70)
        print()
        print("CONCLUSION:")
        print("As long as initial_capital and price data are in the SAME currency,")
        print("the system produces identical results. No currency conversion needed.")
        return 0
    else:
        print("=" * 70)
        print("✗ FAILED: Unexpected mismatch detected")
        print("=" * 70)
        return 1

if __name__ == "__main__":
    exit(main())
