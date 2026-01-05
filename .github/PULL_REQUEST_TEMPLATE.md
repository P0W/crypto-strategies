# Pull Request

## Description

Brief description of what this PR does.

Fixes #(issue number)

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] Performance improvement
- [ ] Code refactoring
- [ ] New strategy
- [ ] Test coverage improvement

## Changes Made

Detailed list of changes:

- Change 1
- Change 2
- Change 3

## Motivation and Context

Why is this change required? What problem does it solve?

## Testing Performed

How has this been tested?

### Test Commands Run

```bash
# Build test
cargo build

# Run tests
cargo test

# Lint checks
cargo fmt --check
cargo clippy

# Backtest (if applicable)
cargo run -- backtest --config ../configs/sample_config.json
```

### Test Results

<details>
<summary>Test output</summary>

```
Paste relevant test output here
```

</details>

### Backtest Results (if strategy change)

| Metric | Before | After |
|--------|--------|-------|
| Sharpe Ratio | X.XX | Y.YY |
| Total Return | X% | Y% |
| Max Drawdown | X% | Y% |
| Win Rate | X% | Y% |

## Performance Impact

- [ ] No performance impact
- [ ] Performance improved (describe how)
- [ ] Performance may be affected (explain why acceptable)

## Breaking Changes

- [ ] No breaking changes
- [ ] Breaking changes (describe migration path below)

<details>
<summary>Breaking change details</summary>

**What breaks:**

**Migration path:**

**Deprecation timeline:**

</details>

## Documentation

- [ ] Code is self-documenting
- [ ] Added/updated code comments
- [ ] Updated README.md
- [ ] Updated CLAUDE.md (if architecture changes)
- [ ] Added/updated configuration examples
- [ ] Updated strategy documentation

## Checklist

### Code Quality

- [ ] My code follows the style guidelines of this project
- [ ] I have performed a self-review of my code
- [ ] I have commented my code, particularly in hard-to-understand areas
- [ ] My changes generate no new warnings
- [ ] Code compiles without errors (`cargo build`)
- [ ] All tests pass (`cargo test`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)

### Testing

- [ ] I have added tests that prove my fix is effective or that my feature works
- [ ] New and existing unit tests pass locally
- [ ] I have tested edge cases
- [ ] I have tested error conditions

### Documentation

- [ ] I have made corresponding changes to the documentation
- [ ] I have updated examples (if applicable)
- [ ] Configuration changes are documented
- [ ] API changes are documented

### Dependencies

- [ ] I have not added new dependencies (or explained why necessary)
- [ ] Dependencies are up to date
- [ ] No security vulnerabilities in dependencies (`cargo audit`)

## Screenshots (if applicable)

Add screenshots for visual changes:

## Related Issues

- Closes #
- Related to #
- Depends on #

## Additional Notes

Any additional information that reviewers should know:

## Reviewer Checklist (for maintainers)

- [ ] Code review completed
- [ ] Tests are adequate
- [ ] Documentation is clear
- [ ] No security concerns
- [ ] Performance is acceptable
- [ ] Breaking changes are justified
- [ ] Backtest results verified (for strategies)

---

**By submitting this PR, I confirm:**
- [ ] I have read the [CONTRIBUTING.md](../CONTRIBUTING.md) guidelines
- [ ] I agree to license my contributions under the MIT License
- [ ] My changes do not introduce security vulnerabilities
- [ ] I have removed any sensitive data (API keys, credentials) from the code
