# Repository Security Policy & Hardening Guidelines

This document outlines the mandatory security baselines enforced across this repository to prevent token abuse, credential theft, and supply chain attacks (such as the *Gheist* campaign).

All developers, automated tools, and AI assistants (e.g., Claude) must strictly adhere to these patterns when writing code, provisioning infrastructure, or configuring workflows.

---

## 1. GitHub Actions Hardening

### Minimum Token Permissions
Never grant broad write permissions to the default `GITHUB_TOKEN`. All workflow files must explicitly declare the minimum permissions required.

*Good Pattern:*
```yaml
permissions:
  contents: read
  id-token: write # Only if using OIDC
```

### Protection Against Log Exfiltration
Attackers bypass GitHub's log masking by encoding secrets (e.g., Base32/Base64) before printing them.
* **Rule:** Never allow workflows to echo or print secrets, encoded or raw, into standard output, debug logs, or build artifacts.
* **Rule:** Do not accept workflows that trigger on unverified branch names (e.g., `ci-sync-*`).

---

## 2. Credential Management

### No Hardcoded Secrets
Under no circumstances should API keys, access tokens, or cloud credentials be hardcoded in the codebase, configuration files, or Dockerfiles.
* Use **GitHub Secrets** for repository-level variables.
* Use **Environment Secrets** for production environments requiring manual approvals.

### OpenID Connect (OIDC) for Cloud Access
Do not store long-lived AWS, Azure, or GCP credentials in GitHub Secrets. Workflows needing cloud access must authenticate dynamically using OIDC trust relationships.

---

## 3. Commit & Branch Security

### Signed Commits Only
To prevent attackers from spoofing developer identities using stolen tokens, all commits must be cryptographically signed.
* Configure your local Git environment with GPG or SSH signing keys.
* Enable **Require signed commits** in the GitHub branch protection rules.

### Branch Protection Rules
The main/default branch must be locked down with the following settings enabled:
* Require a pull request before merging.
* Require approvals from at least one repository administrator or designated code owner.
* Require status checks to pass before merging.

---

## 4. Threat Modeling (Claude/AI Instructions)

When generating CI/CD pipelines or infrastructure code for this repository:
1. **Design Defensively:** Prioritize least-privilege security over convenience.
2. **Review Context:** Ensure no debugging statements or verbosity configurations risk leaking environmental variables.
3. **Validate Triggers:** Restrict `on: pull_request` triggers from forks to prevent arbitrary code execution from external contributors.
