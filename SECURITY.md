# Security Policy

## Supported Versions

Only the latest version on `main` is actively maintained.

## Reporting a Vulnerability

**Do NOT open a public issue for security vulnerabilities.**

Contact: hamza.abdagic@outlook.com

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

## Security Principles

- All secrets and credentials stay LOCAL — never committed to git
- No data exfiltration to external services without explicit user consent
- API keys, tokens, and connection strings belong in config files excluded from version control
- Managed identity preferred over credential files in cloud deployments
