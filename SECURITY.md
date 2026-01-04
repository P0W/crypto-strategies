# Security Policy

## Supported Versions

Security updates are provided for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| main    | :white_check_mark: |
| 0.1.x   | :white_check_mark: |

## Security Considerations

### Trading Software Security

This software handles sensitive data and financial operations. Be aware of:

1. **API Credentials**: Never commit API keys to version control
2. **Private Keys**: Keep exchange API secrets secure
3. **Trade Data**: Trading history may contain sensitive information
4. **Network Security**: Use secure connections for exchange APIs
5. **System Access**: Restrict access to systems running live trading

### Data Protection

- **Environment Variables**: Store credentials in `.env` files (gitignored)
- **Configuration Files**: Don't commit configs with real API keys
- **Log Files**: Logs may contain sensitive data; rotate and secure them
- **Database Files**: SQLite state files contain trade history
- **Backup Security**: Encrypt backups of trading data

## Reporting a Vulnerability

### How to Report

**DO NOT** create a public GitHub issue for security vulnerabilities.

Instead, please report security issues privately:

1. **Email**: Send details to the project maintainer (email in GitHub profile)
2. **Subject**: Start with `[SECURITY]`
3. **Content**: Include detailed description, reproduction steps, and impact

### What to Include

A good security report includes:

```markdown
## Vulnerability Type
(e.g., API key exposure, SQL injection, unauthorized access)

## Affected Component
(e.g., backtester, live trading, exchange client)

## Severity
Critical / High / Medium / Low

## Description
Clear description of the vulnerability

## Steps to Reproduce
1. Step one
2. Step two
3. Step three

## Impact
What can an attacker do with this vulnerability?

## Suggested Fix
If you have ideas on how to fix it

## Additional Context
- Version affected
- Configuration details
- Environment information
```

### Response Timeline

* **Initial Response**: Within 48 hours
* **Status Update**: Within 7 days
* **Fix Timeline**: Depends on severity
  - Critical: 1-3 days
  - High: 1-2 weeks
  - Medium: 2-4 weeks
  - Low: Next release

### Disclosure Process

1. **Report received**: We acknowledge receipt
2. **Investigation**: We assess the vulnerability
3. **Fix development**: We develop and test a fix
4. **Private disclosure**: We may notify key users
5. **Public release**: Fix is released with security advisory
6. **Recognition**: Reporter is credited (if desired)

## Security Best Practices

### For Users

#### API Key Management

```bash
# ✅ GOOD: Use .env files
echo "COINDCX_API_KEY=your_key" >> .env
echo "COINDCX_API_SECRET=your_secret" >> .env

# ❌ BAD: Don't hardcode in config files
{
  "api_key": "hardcoded_key_here"  // Never do this!
}
```

#### File Permissions

```bash
# Secure your .env and state files
chmod 600 .env
chmod 600 *.db
chmod 700 logs/

# Verify .gitignore excludes sensitive files
git status --ignored
```

#### Credential Rotation

- **Regular rotation**: Change API keys quarterly
- **After exposure**: Immediately rotate if keys may be compromised
- **Separate keys**: Use different keys for paper trading and live
- **Minimal permissions**: Grant only required API permissions

#### Network Security

```bash
# Use HTTPS only (built-in to exchange clients)
# Verify SSL certificates (default behavior)
# Consider VPN for live trading
# Use firewall rules to restrict network access
```

### For Contributors

#### Code Review Checklist

Before submitting code that handles:

- [ ] **API Credentials**: Never log or expose keys
- [ ] **User Input**: Validate and sanitize all inputs
- [ ] **File Operations**: Use safe path handling
- [ ] **Database Queries**: Use parameterized queries
- [ ] **Network Requests**: Verify SSL, use timeouts
- [ ] **Error Messages**: Don't leak sensitive info in errors

#### Secure Coding Practices

```rust
// ✅ GOOD: Use environment variables
let api_key = env::var("COINDCX_API_KEY")
    .expect("COINDCX_API_KEY not found");

// ❌ BAD: Hardcoded secrets
let api_key = "sk-123456789";

// ✅ GOOD: Parameterized queries
conn.execute(
    "INSERT INTO trades (symbol, price) VALUES (?1, ?2)",
    params![symbol, price],
)?;

// ❌ BAD: String concatenation
conn.execute(
    &format!("INSERT INTO trades VALUES ('{}', {})", symbol, price)
)?;

// ✅ GOOD: Sanitize logs
tracing::info!("Order placed for {}", symbol);

// ❌ BAD: Logging sensitive data
tracing::info!("API Key: {}", api_key);
```

#### Dependency Security

```bash
# Check for vulnerable dependencies
cargo audit

# Keep dependencies updated
cargo update

# Review dependency changes
cargo tree
```

### For Live Trading

#### Production Security

1. **Separate Environments**:
   - Development: Use paper trading
   - Staging: Test with minimal funds
   - Production: Full capital, maximum security

2. **Access Control**:
   ```bash
   # Run as non-root user
   sudo useradd -m -s /bin/bash trader
   sudo su - trader
   
   # Restrict file access
   chmod 700 ~/crypto-strategies
   ```

3. **Monitoring**:
   - Monitor API rate limits
   - Set up alerts for unusual activity
   - Regular audit of trade logs
   - Track portfolio changes

4. **Backup and Recovery**:
   ```bash
   # Regular backups
   tar czf backup-$(date +%Y%m%d).tar.gz \
       .env trading_state.db logs/
   
   # Encrypted backup
   gpg -c backup-$(date +%Y%m%d).tar.gz
   ```

5. **Kill Switch**:
   - Know how to stop trading immediately
   - Test emergency procedures
   - Have manual override capability

## Known Security Considerations

### Current Implementation

1. **API Authentication**:
   - ✅ HMAC-SHA256 signing for CoinDCX
   - ✅ Credentials from environment variables
   - ✅ No credential logging

2. **Data Storage**:
   - ✅ SQLite with file permissions
   - ⚠️ No database encryption (user responsibility)
   - ✅ Automatic state backups to JSON

3. **Network**:
   - ✅ HTTPS for all API calls
   - ✅ Certificate verification enabled
   - ✅ Timeout protection
   - ✅ Rate limiting

4. **Error Handling**:
   - ✅ Circuit breaker pattern
   - ✅ Graceful degradation
   - ⚠️ Error messages may be verbose (for debugging)

### Areas for Improvement

We're actively working on:

- [ ] Database encryption at rest
- [ ] Audit logging for all trades
- [ ] Enhanced input validation
- [ ] Security-focused integration tests
- [ ] Automated security scanning in CI

## Security Updates

Security fixes are released as:

1. **Patch versions** (0.1.x) for minor issues
2. **Minor versions** (0.x.0) for moderate issues
3. **Immediate hotfixes** for critical vulnerabilities

Subscribe to:
- GitHub Security Advisories
- Release notifications
- This repository's watch list

## Compliance

### Financial Regulations

**Disclaimer**: This software is for educational and research purposes.

- Not licensed financial advice
- No guarantees of profit or performance
- Users responsible for regulatory compliance
- Know your local trading regulations

### Data Privacy

- No telemetry or usage tracking
- All data stays local
- No third-party analytics
- User controls all data

## Responsible Disclosure

We follow coordinated vulnerability disclosure:

1. **Private reporting**: Report to us first
2. **Investigation period**: Allow time for fix
3. **Coordinated release**: Fix and disclosure together
4. **Credit**: Reporter acknowledged (optional)

**Do not**:
- Exploit vulnerabilities
- Access unauthorized data
- Disrupt service for others
- Publicly disclose before fix

## Security Champions

Contributors focused on security:

- Code review for security issues
- Dependency audits
- Security testing
- Documentation updates

Interested in helping? See [CONTRIBUTING.md](CONTRIBUTING.md)

## Resources

### Security Tools

```bash
# Audit dependencies
cargo install cargo-audit
cargo audit

# Security linting
cargo install cargo-deny
cargo deny check

# Outdated dependency check
cargo outdated
```

### External Resources

- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [Rust Security Guidelines](https://anssi-fr.github.io/rust-guide/)
- [CWE Database](https://cwe.mitre.org/)
- [CVE Database](https://cve.mitre.org/)

## Contact

For security-related questions:
- Security issues: Private email to maintainer
- General security questions: GitHub Discussions
- Urgent matters: Tag issue as `security`

---

**Remember: Trading involves financial risk. Secure your systems appropriately.**

Last Updated: January 2026
