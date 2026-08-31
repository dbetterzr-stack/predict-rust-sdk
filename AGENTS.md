# Repository instructions

This repository contains a public, unofficial Rust SDK for Predict.fun.

## Change workflow

- Follow the global GitHub workflow: every tracked change requires an Issue,
  a dedicated `codex/` branch, a pull request, passing checks, and squash merge.
- Never push directly to `main`.
- Keep this crate strategy-neutral. Trading strategies, production account
  names, server aliases, and deployment topology belong in downstream private
  repositories.

## Security

- Never commit API keys, JWTs, wallet addresses tied to production, private
  keys, account material, raw signed authentication messages, or production
  order identifiers.
- Public errors and logs must redact authorization headers, API-key query
  parameters, JWT-bearing WebSocket topics, and private-key material.
- Tests must use deterministic synthetic keys and identifiers only.

## Validation

Before pushing Rust changes, run:

```text
cargo fmt --all -- --check
cargo test --all-targets --all-features
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

Also run `git status --short --ignored` and inspect staged content for secrets.
