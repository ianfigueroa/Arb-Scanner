import sqlite3
import unittest
from contextlib import contextmanager
from pathlib import Path
from uuid import uuid4

from analysis.analyze import count_tracked_blocks, load_data, net_profit_eth


def seed_db(db_path: Path) -> None:
    con = sqlite3.connect(str(db_path))
    con.executescript(
        """
        CREATE TABLE opportunities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
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
            timestamp INTEGER NOT NULL,
            chain TEXT NOT NULL,
            block INTEGER NOT NULL,
            weth_usd REAL NOT NULL
        );
        """
    )
    con.execute(
        """
        INSERT INTO opportunities (timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
        (1_700_000_000, "ethereum", "WETH->USDC->DAI->WETH", 1.0, 0.12, 125_000_000_000_000_000, 4.25),
    )
    con.execute(
        """
        INSERT INTO price_snapshots (timestamp, chain, block, weth_usd)
        VALUES (?, ?, ?, ?)
        """,
        (1_700_000_000, "ethereum", 21_000_000, 3200.0),
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
    def test_load_data_reads_current_schema(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            opps, snaps = load_data(str(db_path))

            self.assertEqual(list(opps["path"]), ["WETH->USDC->DAI->WETH"])
            self.assertIn("net_wei", opps.columns)
            self.assertEqual(list(snaps["block"]), [21_000_000])

    def test_count_tracked_blocks_uses_block_column(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            _, snaps = load_data(str(db_path))

            self.assertEqual(count_tracked_blocks(snaps), 1)

    def test_net_profit_eth_uses_net_wei_column(self) -> None:
        with temp_db_path() as db_path:
            seed_db(db_path)

            opps, _ = load_data(str(db_path))

            net_eth = net_profit_eth(opps)
            self.assertAlmostEqual(float(net_eth.iloc[0]), 0.125)


if __name__ == "__main__":
    unittest.main()
