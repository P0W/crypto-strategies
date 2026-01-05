# Contributing to Crypto Strategies

Thank you for your interest in contributing to this project! This document provides guidelines and instructions for contributing.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [How to Contribute](#how-to-contribute)
- [Code Standards](#code-standards)
- [Testing Guidelines](#testing-guidelines)
- [Submitting Changes](#submitting-changes)
- [Community Guidelines](#community-guidelines)

## Getting Started

### Prerequisites

- **Rust** (1.70+): Install from [rustup.rs](https://rustup.rs/)
- **Git**: Version control
- **Basic understanding** of trading concepts (optional but helpful)

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/crypto-strategies.git
   cd crypto-strategies
   ```
3. Add upstream remote:
   ```bash
   git remote add upstream https://github.com/P0W/crypto-strategies.git
   ```

## Development Setup

### Rust Implementation (Primary Focus)

```bash
cd rust

# Install dependencies and build
cargo build

# Run tests
cargo test

# Run with sample config
cargo run -- backtest --config ../configs/sample_config.json
```

### Environment Configuration

```bash
# Copy the example environment file
cp .env.example .env

# Add your API credentials (optional for backtesting)
# COINDCX_API_KEY=your_key_here
# COINDCX_API_SECRET=your_secret_here
```

## How to Contribute

### Reporting Bugs

Use the [Bug Report](/.github/ISSUE_TEMPLATE/bug_report.md) template and include:

- **Description**: Clear description of the bug
- **Steps to Reproduce**: Minimal steps to reproduce the issue
- **Expected Behavior**: What should happen
- **Actual Behavior**: What actually happens
- **Environment**: OS, Rust version, command used
- **Logs**: Relevant log output or error messages

### Suggesting Features

Use the [Feature Request](/.github/ISSUE_TEMPLATE/feature_request.md) template and include:

- **Problem**: What problem does this solve?
- **Proposed Solution**: How would you solve it?
- **Alternatives**: Other approaches you've considered
- **Use Case**: Concrete example of how this helps

### Creating New Strategies

We encourage community-contributed strategies! See [examples/custom_strategy/](examples/custom_strategy/) for a complete guide.

**Quick checklist:**
1. Create new module in `rust/src/strategies/your_strategy/`
2. Implement the `Strategy` trait
3. Add configuration struct
4. Register in strategy factory
5. Add tests
6. Document parameters and edge
7. Share backtest results

## Code Standards

### Rust Code Style

- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings
- Add doc comments for public APIs

```rust
/// Calculate the Average True Range (ATR) indicator.
///
/// # Arguments
///
/// * `candles` - Slice of OHLCV candles
/// * `period` - Lookback period for ATR
///
/// # Returns
///
/// Vector of ATR values (same length as input)
pub fn atr(candles: &[Candle], period: usize) -> Vec<f64> {
    // Implementation
}
```

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add Bollinger Bands breakout strategy
fix: correct ATR calculation for gap candles
docs: update strategy configuration examples
test: add integration tests for optimizer
refactor: simplify risk manager position sizing
perf: optimize indicator calculations with SIMD
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `test`: Adding/updating tests
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `chore`: Maintenance tasks

### Code Organization

- Keep functions small and focused (< 50 lines)
- Extract complex logic into helper functions
- Use descriptive variable names
- Avoid deep nesting (max 3 levels)
- Prefer composition over inheritance

## Testing Guidelines

### Unit Tests

Place tests in the same file as the code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atr_calculation() {
        let candles = vec![/* test data */];
        let atr = atr(&candles, 14);
        assert!(!atr.is_empty());
    }
}
```

### Integration Tests

Place in `rust/tests/`:

```rust
// rust/tests/strategy_integration.rs
use crypto_strategies::*;

#[test]
fn test_volatility_regime_backtest() {
    // Load config, run backtest, verify metrics
}
```

### Test Coverage

- **Critical paths**: 100% coverage (risk management, position sizing)
- **Strategies**: Test signal generation, stop loss, take profit
- **Indicators**: Test edge cases (empty data, NaN, single value)
- **Exchange clients**: Mock API responses

### Running Tests

```bash
# All tests
cargo test

# Specific module
cargo test strategies::volatility_regime

# Show output
cargo test -- --nocapture

# With optimizations
cargo test --release
```

## Submitting Changes

### Pull Request Process

1. **Create a branch** from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** and commit:
   ```bash
   git add .
   git commit -m "feat: add your feature"
   ```

3. **Keep your branch updated**:
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

4. **Push to your fork**:
   ```bash
   git push origin feature/your-feature-name
   ```

5. **Open a Pull Request** on GitHub

### Pull Request Checklist

- [ ] Code compiles without warnings (`cargo build`)
- [ ] All tests pass (`cargo test`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Documentation is updated
- [ ] Commit messages follow conventions
- [ ] PR description explains changes clearly
- [ ] Breaking changes are documented

### PR Description Template

```markdown
## Description
Brief description of changes

## Motivation
Why is this change needed?

## Changes Made
- Item 1
- Item 2

## Testing
How was this tested?

## Checklist
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] No breaking changes (or documented)
```

## Community Guidelines

### Be Respectful

- **Constructive feedback**: Focus on code, not people
- **Assume good intentions**: We're all learning
- **Be patient**: Contributors have different experience levels
- **No discrimination**: Based on experience, identity, or background

### Ask for Help

- **GitHub Discussions**: For general questions
- **Issues**: For bug reports and feature requests
- **Pull Requests**: For code review and discussion
- **Discord/Slack**: (if available) Real-time chat

### Recognition

All contributors are recognized in:
- Git commit history
- Release notes
- README acknowledgments (for significant contributions)

## Development Workflow

### Typical Contribution Flow

1. **Find an issue** or create one
2. **Comment** that you're working on it
3. **Fork** and create a branch
4. **Develop** and test locally
5. **Submit PR** with clear description
6. **Address feedback** from maintainers
7. **Merge** after approval

### Priority Areas

We especially welcome contributions in:

- 🎯 **New strategies**: Novel trading approaches with backtested results
- 📊 **Indicators**: Additional technical indicators
- 🧪 **Testing**: Improve test coverage
- 📖 **Documentation**: Tutorials, examples, guides
- 🐛 **Bug fixes**: Issues labeled "good first issue"
- ⚡ **Performance**: Optimization opportunities

## Strategy Contribution Guidelines

### Requirements for New Strategies

1. **Working implementation** with config
2. **Backtest results** on provided data (min 2 years)
3. **Documentation**:
   - Strategy logic and edge hypothesis
   - Parameter descriptions
   - Best timeframes/markets
   - Risk characteristics
4. **Tests**: Signal generation and exit logic
5. **Example config**: Ready-to-run JSON

### Strategy Quality Standards

- **Sharpe Ratio**: > 0.5 preferred
- **Max Drawdown**: < 30% preferred
- **Win Rate**: Realistic (don't overfit)
- **Trade Count**: Sufficient sample size (> 30 trades)
- **Robustness**: Works across multiple symbols

### Evaluation Process

1. Maintainers review code quality
2. Verify backtest results independently
3. Test on out-of-sample data
4. Check for overfitting
5. Merge if meets quality standards

## License

By contributing, you agree that your contributions will be licensed under the MIT License.

## Questions?

- Check existing [Issues](https://github.com/P0W/crypto-strategies/issues)
- Read the [Documentation](README.md)
- Open a new issue if needed

---

**Thank you for contributing! Together we build better trading systems. 🚀**
