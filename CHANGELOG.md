# Changelog

All notable changes to this project will be documented in this file.

## 0.1.0 - 2026-03-21

- Added a read-only multi-chain arbitrage scanner in Rust for Ethereum, Arbitrum, Base, and Polygon.
- Added session-aware SQLite persistence with per-run session records, session-tagged opportunities, and session-tagged price snapshots.
- Added migration support for legacy databases by backfilling older rows into a synthetic `legacy` session.
- Added Python analytics for latest-session analysis, explicit `--session` selection, `--all` aggregate analysis, and `--list-sessions`.
- Added session-scoped chart output directories under `analysis/output/<scope>/`.
- Added public repository metadata, architecture documentation, CI guidance, and verification commands for contributor confidence.
