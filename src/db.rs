use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::WrapErr;
use rusqlite::{params, Connection};

use crate::arb::u256_to_f64;
use crate::types::ArbOpportunity;

// ─── Database handle ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OpportunityDb {
    conn: Arc<Mutex<Connection>>,
}

impl OpportunityDb {
    pub fn open(path: &str) -> eyre::Result<Self> {
        let conn =
            Connection::open(path).wrap_err_with(|| format!("failed to open database at {path}"))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> eyre::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS opportunities (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp     INTEGER NOT NULL,
                chain         TEXT NOT NULL,
                path          TEXT NOT NULL,
                input_eth     REAL NOT NULL,
                roi_pct       REAL NOT NULL,
                net_wei       INTEGER NOT NULL,
                gas_cost_usd  REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS price_snapshots (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp     INTEGER NOT NULL,
                chain         TEXT NOT NULL,
                block         INTEGER NOT NULL,
                weth_usd      REAL NOT NULL
            );",
        )
        .wrap_err("failed to create tables")?;
        Ok(())
    }

    pub fn insert_opportunity(&self, opp: &ArbOpportunity) -> eyre::Result<()> {
        let input_eth = u256_to_f64(opp.input_weth) / 1e18;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO opportunities (timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                opp.timestamp as i64,
                opp.chain.name(),
                opp.path,
                input_eth,
                opp.roi_pct,
                opp.estimated_net_after_gas as i64,
                opp.gas_cost_usd,
            ],
        )
        .wrap_err("insert_opportunity failed")?;
        Ok(())
    }

    pub fn insert_price_snapshot(&self, chain: &str, block: u64, weth_usd: f64) -> eyre::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO price_snapshots (timestamp, chain, block, weth_usd)
             VALUES (?1, ?2, ?3, ?4)",
            params![timestamp as i64, chain, block as i64, weth_usd],
        )
        .wrap_err("insert_price_snapshot failed")?;
        Ok(())
    }

    pub fn top_paths_by_count(&self, n: usize) -> eyre::Result<Vec<(String, u64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT path, COUNT(*) as cnt FROM opportunities
                 GROUP BY path ORDER BY cnt DESC LIMIT ?1",
            )
            .wrap_err("prepare top_paths_by_count failed")?;
        let rows = stmt
            .query_map([n as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .wrap_err("query top_paths_by_count failed")?
            .collect::<Result<Vec<_>, _>>()
            .wrap_err("collect top_paths_by_count failed")?;
        Ok(rows.into_iter().map(|(p, c)| (p, c as u64)).collect())
    }

    pub fn top_paths_by_avg_roi(&self, n: usize) -> eyre::Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT path, AVG(roi_pct) as avg_roi FROM opportunities
                 GROUP BY path ORDER BY avg_roi DESC LIMIT ?1",
            )
            .wrap_err("prepare top_paths_by_avg_roi failed")?;
        let rows = stmt
            .query_map([n as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .wrap_err("query top_paths_by_avg_roi failed")?
            .collect::<Result<Vec<_>, _>>()
            .wrap_err("collect top_paths_by_avg_roi failed")?;
        Ok(rows)
    }

    pub fn export_csv(&self, out_path: &str) -> eyre::Result<()> {
        use std::io::Write;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd
                 FROM opportunities ORDER BY id",
            )
            .wrap_err("prepare export_csv failed")?;
        let mut file = std::fs::File::create(out_path)
            .wrap_err_with(|| format!("failed to create CSV file at {out_path}"))?;
        writeln!(file, "timestamp,chain,path,input_eth,roi_pct,net_wei,gas_cost_usd")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, f64>(6)?,
                ))
            })
            .wrap_err("query export_csv failed")?;
        for row in rows {
            let (ts, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd) =
                row.wrap_err("row error in export_csv")?;
            writeln!(file, "{ts},{chain},{path},{input_eth},{roi_pct},{net_wei},{gas_cost_usd}")?;
        }
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArbOpportunity, ChainId};
    use ethers::types::U256;

    fn make_opp(path: &'static str, roi: f64) -> ArbOpportunity {
        ArbOpportunity {
            path,
            chain: ChainId::Ethereum,
            input_weth: U256::from(1_000_000_000_000_000_000u64),
            estimated_net_after_gas: 1000,
            roi_pct: roi,
            gas_cost_usd: 5.0,
            timestamp: 1_700_000_000,
        }
    }

    #[test]
    fn test_insert_and_query_opportunity() {
        let db = OpportunityDb::open(":memory:").unwrap();
        db.insert_opportunity(&make_opp("A→B→C", 0.01)).unwrap();
        let count = {
            let conn = db.conn.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM opportunities", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
        };
        assert_eq!(count, 1);
    }

    #[test]
    fn test_top_paths_ordering() {
        let db = OpportunityDb::open(":memory:").unwrap();
        db.insert_opportunity(&make_opp("PATH_A", 0.01)).unwrap();
        db.insert_opportunity(&make_opp("PATH_B", 0.01)).unwrap();
        db.insert_opportunity(&make_opp("PATH_B", 0.01)).unwrap();
        db.insert_opportunity(&make_opp("PATH_C", 0.01)).unwrap();
        db.insert_opportunity(&make_opp("PATH_C", 0.01)).unwrap();
        db.insert_opportunity(&make_opp("PATH_C", 0.01)).unwrap();

        let top = db.top_paths_by_count(3).unwrap();
        assert_eq!(top[0].0, "PATH_C");
        assert_eq!(top[0].1, 3);
        assert_eq!(top[1].0, "PATH_B");
        assert_eq!(top[1].1, 2);
        assert_eq!(top[2].0, "PATH_A");
        assert_eq!(top[2].1, 1);
    }

    #[test]
    fn test_export_csv_creates_file() {
        let db = OpportunityDb::open(":memory:").unwrap();
        db.insert_opportunity(&make_opp("A→B→C", 0.01)).unwrap();
        let path = std::env::temp_dir().join("test_arb_export.csv");
        let path_str = path.to_str().unwrap();
        db.export_csv(path_str).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("timestamp,chain,path,"));
        assert!(content.lines().count() >= 2);
    }
}
