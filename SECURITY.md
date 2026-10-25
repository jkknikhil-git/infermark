# Security Policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report privately through GitHub: **Security → Report a vulnerability** on
[jkknikhil-git/infermark](https://github.com/jkknikhil-git/infermark/security/advisories/new),
or email jkk.nikhil@gmail.com with "InferMark security" in the subject.

You can expect an acknowledgement within 5 days. We'll keep you updated, credit you in the release notes
(unless you prefer otherwise) and publish an advisory once a fix is released.

## Supported versions

Until 1.0, only the latest release receives security fixes.

## Security design

InferMark is a local, offline-first desktop and mobile app; there is no InferMark server in v1.

| Area | What we do |
| --- | --- |
| App shell | Tauri 2 with a strict Content-Security-Policy, no remote content, and only the `core:default` capability. The UI cannot run shell commands or read arbitrary files. |
| Commands | Every command the UI can call validates its input (e.g. result files can only be opened from the results folder). |
| Test packs & profiles | Pure data (TOML / JSONL), strictly parsed. They are never executed. |
| Model-generated code | Coding tests run generated code in an isolated, time- and memory-limited sandbox without network access. |
| Downloads | HTTPS only, allow-listed hosts, SHA-256 verified before use. |
| Updates | Releases are signed; the updater only installs signature-verified builds. |
| Dependencies | Lockfiles committed; `cargo-deny`, `npm audit`, Dependabot and CodeQL run in CI. |
| Secrets | Optional API keys (for LLM-as-judge) are stored in the OS keychain, never in files or results. |

See [PRIVACY.md](PRIVACY.md) for what data InferMark stores.
