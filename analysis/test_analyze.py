import sqlite3
import unittest
from contextlib import contextmanager
from pathlib import Path
from uuid import uuid4

import matplotlib.pyplot as plt

from analysis.analyze import (
    annotate_empty_axis,
    count_tracked_blocks,
    list_sessions,
    load_data,
    net_profit_eth,
    resolve_session_scope,
    session_overview_stats,
)


def seed_db(db_path: Path) -> None:
    con = sqlite3.connect(str(db_path))
    con.executescript(
        """
        CREATE TABLE sessions (
            session_id TEXT PRIMARY KEY,
            started_at INTEGER NOT NULL,
            ended_at INTEGER NULL,
            active_chains TEXT NOT NULL,
            cross_chain_threshold_pct REAL NOT NULL,
            status TEXT NOT NULL
        );

        CREATE TABLE opportunities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            chain TEXT NOT NULL,
            path TEXT NOT NULL,
            input_eth REAL NOT NULL,
            roi_pct REAL NOT NULL,
            net_wei INTEGER NOT NULL,
            gas_cost_usd REAL NOT NULL
        );

        CREATE TABLE price_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            chain TEXT NOT NULL,
            block INTEGER NOT NULL,
            weth_usd REAL NOT NULL
        );
        """
    )
    sessions = [
        ("session-older", 1_700_000_000, 1_700_000_300, "ethereum", 0.1, "completed"),
        ("session-latest", 1_700_000_600, None, "ethereum,base", 0.2, "active"),
    ]
    con.executemany(
        """
        INSERT INTO sessions (session_id, started_at, ended_at, active_chains, cross_chain_threshold_pct, status)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        sessions,
    )
    opportunities = [
        ("session-older", 1_700_000_000, "ethereum", "WETH->USDC->DAI->WETH", 1.0, 0.12, 125_000_000_000_000_000, 4.25),
        ("session-latest", 1_700_000_600, "base", "WETH->DAI->USDC->WETH", 1.0, 0.18, 225_000_000_000_000_000, 0.55),
    ]
    con.executemany(
        """
        INSERT INTO opportunities (session_id, timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        opportunities,
    )
    snapshots = [
        ("session-older", 1_700_000_000, "ethereum", 21_000_000, 3200.0),
        ("session-latest", 1_700_000_600, "ethereum", 21_000_001, 3205.0),
        ("session-latest", 1_700_000_900, "base", 21_000_002, 3210.0),
    ]
    con.executemany(
        """
        INSERT INTO price_snapshots (session_id, timestamp, chain, block, weth_usd)
        VALUES (?, ?, ?, ?, ?)
        """,
        snapshots,
    )
    con.commit()
    con.close()


@contextmanager
def temp_db_path():
    db_path = Path(__file__).parent / f"test_{uuid4().hex}.db"
    try:
        yield db_path
    finally:
        db_path.unlink(missing_ok=True)


class AnalyzeTests(unittest.TestCase):
    def test_load_data_defaults_to_latest_session(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            scope = resolve_session_scope(str(db_path))
            opps, snaps = load_data(str(db_path), session_id=scope)

            self.assertEqual(scope, "session-latest")
            self.assertEqual(list(opps["session_id"]), ["session-latest"])
            self.assertEqual(sorted(snaps["chain"].unique().tolist()), ["base", "ethereum"])

    def test_load_data_reads_current_schema_for_selected_session(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            opps, snaps = load_data(str(db_path), session_id="session-older")

            self.assertEqual(list(opps["path"]), ["WETH->USDC->DAI->WETH"])
            self.assertIn("net_wei", opps.columns)
            self.assertEqual(list(snaps["block"]), [21_000_000])

    def test_load_data_all_scope_returns_all_rows(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            opps, snaps = load_data(str(db_path), include_all=True)

            self.assertEqual(len(opps), 2)
            self.assertEqual(len(snaps), 3)

    def test_count_tracked_blocks_uses_block_column(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            _, snaps = load_data(str(db_path), session_id="session-latest")

            self.assertEqual(count_tracked_blocks(snaps), 2)

    def test_net_profit_eth_uses_net_wei_column(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            opps, _ = load_data(str(db_path), session_id="session-latest")

            net_eth = net_profit_eth(opps)
            self.assertAlmostEqual(float(net_eth.iloc[0]), 0.225)

    def test_session_overview_uses_snapshots_when_no_opportunities(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            con = sqlite3.connect(str(db_path))
            con.execute("DELETE FROM opportunities WHERE session_id = ?", ("session-latest",))
            con.commit()
            con.close()

            opps, snaps = load_data(str(db_path), session_id="session-latest")
            overview = session_overview_stats(opps, snaps)

            self.assertEqual(overview["total_opportunities"], 0)
            self.assertEqual(overview["chains_active"], 2)
            self.assertEqual(overview["blocks_tracked"], 2)
            self.assertEqual(
                overview["time_range"],
                "2023-11-14 22:23 UTC -> 2023-11-14 22:28 UTC",
            )

    def test_list_sessions_returns_ordered_rows(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            sessions = list_sessions(str(db_path))

            self.assertEqual([session["session_id"] for session in sessions], ["session-latest", "session-older"])
            self.assertEqual([session["status"] for session in sessions], ["active", "completed"])

    def test_annotate_empty_axis_adds_placeholder_message(self) -> None:
        fig, ax = plt.subplots()
        try:
            annotate_empty_axis(ax, "No opportunities recorded for this session.")

            self.assertEqual(len(ax.texts), 1)
            self.assertEqual(
                ax.texts[0].get_text(),
                "No opportunities recorded for this session.",
            )
        finally:
            plt.close(fig)


if __name__ == "__main__":
    unittest.main()
