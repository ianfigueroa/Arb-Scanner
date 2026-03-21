# Architecture

`Arb Scanner` is split into a live Rust runtime and a post-run Python analysis layer.

```mermaid
flowchart LR
    subgraph Runtime["Rust Runtime"]
        A["main.rs\nstartup + task orchestration"]
        B["pools.rs\nfactory resolution\nverification\nsubscriptions"]
        C["arb.rs\npath execution\nprofit estimation"]
        D["cross_chain.rs\nspread monitor"]
        E["db.rs\nSQLite persistence"]
    end

    subgraph Chains["WebSocket RPC Providers"]
        F["Ethereum"]
        G["Arbitrum"]
        H["Base"]
        I["Polygon"]
    end

    subgraph Data["SQLite"]
        J["sessions"]
        K["opportunities"]
        L["price_snapshots"]
    end

    subgraph Analytics["Python Analysis"]
        M["analysis/analyze.py"]
        N["PNG charts\nsummary tables"]
    end

    A --> B
    A --> C
    A --> D
    B --> F
    B --> G
    B --> H
    B --> I
    C --> E
    D --> E
    E --> J
    E --> K
    E --> L
    M --> J
    M --> K
    M --> L
    M --> N
```

## Runtime Responsibilities

- `main.rs` loads configuration, creates a new session record, starts one chain task per enabled WebSocket RPC, and prints the session summary on shutdown.
- `pools.rs` resolves pools, verifies token ordering, bootstraps reserve state, subscribes to pool events, and refreshes stale pools.
- `arb.rs` runs the configured triangular paths and estimates profitability after gas.
- `cross_chain.rs` compares per-chain WETH prices and emits spread alerts.
- `db.rs` stores session metadata, profitable opportunities, and price snapshots in SQLite.

## Data Model

- `sessions`: one row per scanner run with `session_id`, `started_at`, `ended_at`, active chains, and the configured cross-chain alert threshold.
- `opportunities`: profitable opportunities only, tagged with `session_id`.
- `price_snapshots`: block-level WETH reference prices, tagged with `session_id`.

Legacy databases are migrated in place. Older rows are backfilled into a synthetic `legacy` session so the data remains analyzable.

## Analysis Responsibilities

- `analysis/analyze.py` reads one session by default.
- `--session <id>` selects an explicit session.
- `--all` analyzes the full database.
- `--list-sessions` prints available sessions and exits.

Charts are written into `analysis/output/<scope>/`, where scope is either a real `session_id` or `all`.
