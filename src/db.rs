use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::{eyre, WrapErr};
use rusqlite::{params, Connection, OptionalExtension};

use crate::arb::u256_to_f64;
use crate::types::{ArbOpportunity, SessionInfo};

const LEGACY_SESSION_ID: &str = "legacy";
const SESSION_STATUS_ACTIVE: &str = "active";
const SESSION_STATUS_COMPLETED: &str = "completed";
const SESSION_STATUS_RECOVERED: &str = "recovered";

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRecord {
    pub session_id: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub active_chains: String,
    pub cross_chain_threshold_pct: f64,
    pub status: String,
}

#[derive(Clone)]
pub struct OpportunityDb {
    conn: Arc<Mutex<Connection>>,
}

fn checked_i128_to_i64(value: i128, field: &str) -> eyre::Result<i64> {
    i64::try_from(value).map_err(|_| eyre!("{field} out of SQLite INTEGER range: {value}"))
}

fn checked_u64_to_i64(value: u64, field: &str) -> eyre::Result<i64> {
    i64::try_from(value).map_err(|_| eyre!("{field} out of SQLite INTEGER range: {value}"))
}

fn checked_i64_to_u64(value: i64, field: &str) -> eyre::Result<u64> {
    u64::try_from(value).map_err(|_| eyre!("{field} out of u64 range: {value}"))
}

impl OpportunityDb {
    pub fn open(path: &str) -> eyre::Result<Self> {
        let conn = Connection::open(path)
            .wrap_err_with(|| format!("failed to open database at {path}"))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> eyre::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_id                 TEXT PRIMARY KEY,
                started_at                 INTEGER NOT NULL,
                ended_at                   INTEGER NULL,
                active_chains              TEXT NOT NULL,
                cross_chain_threshold_pct  REAL NOT NULL,
                status                     TEXT NOT NULL DEFAULT 'active'
            );
            CREATE TABLE IF NOT EXISTS opportunities (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id    TEXT NOT NULL,
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
                session_id    TEXT NOT NULL,
                timestamp     INTEGER NOT NULL,
                chain         TEXT NOT NULL,
                block         INTEGER NOT NULL,
                weth_usd      REAL NOT NULL
            );",
        )
        .wrap_err("failed to create tables")?;

        ensure_column(&conn, "sessions", "status", "TEXT")?;
        ensure_column(&conn, "opportunities", "session_id", "TEXT")?;
        ensure_column(&conn, "price_snapshots", "session_id", "TEXT")?;
        backfill_legacy_session(&conn)?;
        backfill_session_status(&conn)?;
        purge_orphaned_legacy_session(&conn)?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_opportunities_session_id ON opportunities(session_id);
             CREATE INDEX IF NOT EXISTS idx_price_snapshots_session_id ON price_snapshots(session_id);
             CREATE INDEX IF NOT EXISTS idx_price_snapshots_session_block
                 ON price_snapshots(session_id, chain, block);",
        )
        .wrap_err("failed to create session indexes")?;
        Ok(())
    }

    pub fn create_session(
        &self,
        session: &SessionInfo,
        active_chains: &str,
        cross_chain_threshold_pct: f64,
    ) -> eyre::Result<()> {
        let started_at = checked_u64_to_i64(session.started_at, "started_at")?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (
                session_id, started_at, ended_at, active_chains, cross_chain_threshold_pct, status
            ) VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            params![
                session.id,
                started_at,
                active_chains,
                cross_chain_threshold_pct,
                SESSION_STATUS_ACTIVE,
            ],
        )
        .wrap_err("create_session failed")?;
        Ok(())
    }

    pub fn close_session(&self, session_id: &str, ended_at: u64) -> eyre::Result<()> {
        let ended_at = checked_u64_to_i64(ended_at, "ended_at")?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET ended_at = ?1, status = ?2 WHERE session_id = ?3",
            params![ended_at, SESSION_STATUS_COMPLETED, session_id],
        )
        .wrap_err("close_session failed")?;
        Ok(())
    }

    pub fn recover_incomplete_sessions(&self, ended_at: u64) -> eyre::Result<u64> {
        let ended_at = checked_u64_to_i64(ended_at, "ended_at")?;
        let conn = self.conn.lock().unwrap();
        let updated = conn
            .execute(
                "UPDATE sessions
                 SET ended_at = COALESCE(ended_at, ?1), status = ?2
                 WHERE status = ?3
                    OR ((status IS NULL OR TRIM(status) = '') AND ended_at IS NULL)",
                params![ended_at, SESSION_STATUS_RECOVERED, SESSION_STATUS_ACTIVE,],
            )
            .wrap_err("recover_incomplete_sessions failed")?;
        Ok(updated as u64)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn latest_session_id(&self) -> eyre::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let session_id = conn
            .query_row(
                "SELECT session_id FROM sessions
                 ORDER BY started_at DESC, session_id DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .wrap_err("latest_session_id failed")?;
        Ok(session_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_sessions(&self) -> eyre::Result<Vec<SessionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT session_id, started_at, ended_at, active_chains, cross_chain_threshold_pct, status
                 FROM sessions
                 ORDER BY started_at DESC, session_id DESC",
            )
            .wrap_err("prepare list_sessions failed")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SessionRecord {
                    session_id: row.get::<_, String>(0)?,
                    started_at: checked_i64_to_u64(row.get::<_, i64>(1)?, "started_at")
                        .map_err(to_sql_error)?,
                    ended_at: row
                        .get::<_, Option<i64>>(2)?
                        .map(|value| checked_i64_to_u64(value, "ended_at").map_err(to_sql_error))
                        .transpose()?,
                    active_chains: row.get::<_, String>(3)?,
                    cross_chain_threshold_pct: row.get::<_, f64>(4)?,
                    status: row.get::<_, String>(5)?,
                })
            })
            .wrap_err("query list_sessions failed")?
            .collect::<Result<Vec<_>, _>>()
            .wrap_err("collect list_sessions failed")?;
        Ok(rows)
    }

    pub fn insert_opportunity(&self, session_id: &str, opp: &ArbOpportunity) -> eyre::Result<()> {
        let input_eth = u256_to_f64(opp.input_weth) / 1e18;
        let timestamp = checked_u64_to_i64(opp.timestamp, "timestamp")?;
        let net_wei = checked_i128_to_i64(opp.estimated_net_after_gas, "net_wei")?;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO opportunities (
                session_id, timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id,
                timestamp,
                opp.chain.name(),
                opp.path,
                input_eth,
                opp.roi_pct,
                net_wei,
                opp.gas_cost_usd,
            ],
        )
        .wrap_err("insert_opportunity failed")?;
        Ok(())
    }

    pub fn insert_price_snapshot(
        &self,
        session_id: &str,
        chain: &str,
        block: u64,
        weth_usd: f64,
    ) -> eyre::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let timestamp = checked_u64_to_i64(timestamp, "timestamp")?;
        let block = checked_u64_to_i64(block, "block")?;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO price_snapshots (session_id, timestamp, chain, block, weth_usd)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, timestamp, chain, block, weth_usd],
        )
        .wrap_err("insert_price_snapshot failed")?;
        Ok(())
    }

    pub fn top_paths_by_count(
        &self,
        session_id: Option<&str>,
        n: usize,
    ) -> eyre::Result<Vec<(String, u64)>> {
        let conn = self.conn.lock().unwrap();
        let rows = if let Some(session_id) = session_id {
            let mut stmt = conn
                .prepare(
                    "SELECT path, COUNT(*) as cnt FROM opportunities
                     WHERE session_id = ?1
                     GROUP BY path ORDER BY cnt DESC LIMIT ?2",
                )
                .wrap_err("prepare top_paths_by_count failed")?;
            collect_path_counts(&mut stmt, params![session_id, n as i64])?
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT path, COUNT(*) as cnt FROM opportunities
                     GROUP BY path ORDER BY cnt DESC LIMIT ?1",
                )
                .wrap_err("prepare top_paths_by_count failed")?;
            collect_path_counts(&mut stmt, [n as i64])?
        };
        Ok(rows
            .into_iter()
            .map(|(path, count)| (path, count as u64))
            .collect())
    }

    pub fn top_paths_by_avg_roi(
        &self,
        session_id: Option<&str>,
        n: usize,
    ) -> eyre::Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().unwrap();
        let rows = if let Some(session_id) = session_id {
            let mut stmt = conn
                .prepare(
                    "SELECT path, AVG(roi_pct) as avg_roi FROM opportunities
                     WHERE session_id = ?1
                     GROUP BY path ORDER BY avg_roi DESC LIMIT ?2",
                )
                .wrap_err("prepare top_paths_by_avg_roi failed")?;
            collect_path_roi_rows(&mut stmt, params![session_id, n as i64])?
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT path, AVG(roi_pct) as avg_roi FROM opportunities
                     GROUP BY path ORDER BY avg_roi DESC LIMIT ?1",
                )
                .wrap_err("prepare top_paths_by_avg_roi failed")?;
            collect_path_roi_rows(&mut stmt, [n as i64])?
        };
        Ok(rows)
    }

    pub fn export_csv(&self, session_id: Option<&str>, out_path: &str) -> eyre::Result<()> {
        use std::io::Write;

        let conn = self.conn.lock().unwrap();
        let mut file = std::fs::File::create(out_path)
            .wrap_err_with(|| format!("failed to create CSV file at {out_path}"))?;
        writeln!(
            file,
            "session_id,timestamp,chain,path,input_eth,roi_pct,net_wei,gas_cost_usd"
        )?;
        if let Some(session_id) = session_id {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd
                     FROM opportunities
                     WHERE session_id = ?1
                     ORDER BY id",
                )
                .wrap_err("prepare export_csv failed")?;
            let rows = stmt
                .query_map([session_id], read_export_row)
                .wrap_err("query export_csv failed")?;
            for row in rows {
                let (session_id, ts, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd) =
                    row.wrap_err("row error in export_csv")?;
                writeln!(
                    file,
                    "{session_id},{ts},{chain},{path},{input_eth},{roi_pct},{net_wei},{gas_cost_usd}"
                )?;
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd
                     FROM opportunities ORDER BY id",
                )
                .wrap_err("prepare export_csv failed")?;
            let rows = stmt
                .query_map([], read_export_row)
                .wrap_err("query export_csv failed")?;
            for row in rows {
                let (session_id, ts, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd) =
                    row.wrap_err("row error in export_csv")?;
                writeln!(
                    file,
                    "{session_id},{ts},{chain},{path},{input_eth},{roi_pct},{net_wei},{gas_cost_usd}"
                )?;
            }
        }
        Ok(())
    }
}

type ExportRow = (String, i64, String, String, f64, f64, i64, f64);

fn read_export_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExportRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, f64>(4)?,
        row.get::<_, f64>(5)?,
        row.get::<_, i64>(6)?,
        row.get::<_, f64>(7)?,
    ))
}

fn collect_path_counts<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> eyre::Result<Vec<(String, i64)>>
where
    P: rusqlite::Params,
{
    stmt.query_map(params, |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })
    .wrap_err("query top_paths_by_count failed")?
    .collect::<Result<Vec<_>, _>>()
    .wrap_err("collect top_paths_by_count failed")
}

fn collect_path_roi_rows<P>(
    stmt: &mut rusqlite::Statement<'_>,
    params: P,
) -> eyre::Result<Vec<(String, f64)>>
where
    P: rusqlite::Params,
{
    stmt.query_map(params, |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })
    .wrap_err("query top_paths_by_avg_roi failed")?
    .collect::<Result<Vec<_>, _>>()
    .wrap_err("collect top_paths_by_avg_roi failed")
}

fn to_sql_error(report: eyre::Report) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Integer,
        Box::new(std::io::Error::other(report.to_string())),
    )
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> eyre::Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn
        .prepare(&pragma)
        .wrap_err_with(|| format!("prepare table_info failed for {table}"))?;
    let mut rows = stmt
        .query([])
        .wrap_err_with(|| format!("query table_info failed for {table}"))?;
    while let Some(row) = rows.next()? {
        let existing = row.get::<_, String>(1)?;
        if existing == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
        [],
    )
    .wrap_err_with(|| format!("add column {column} to {table} failed"))?;
    Ok(())
}

fn backfill_legacy_session(conn: &Connection) -> eyre::Result<()> {
    if !legacy_backfill_needed(conn)? {
        return Ok(());
    }
    let metadata = legacy_session_metadata(conn)?;
    if metadata.started_at == 0 && metadata.ended_at.is_none() && metadata.active_chains == "legacy"
    {
        return Ok(());
    }
    let started_at = checked_u64_to_i64(metadata.started_at, "started_at")?;
    let ended_at = metadata
        .ended_at
        .map(|value| checked_u64_to_i64(value, "ended_at"))
        .transpose()?;
    let status = if metadata.ended_at.is_some() {
        SESSION_STATUS_COMPLETED
    } else {
        SESSION_STATUS_ACTIVE
    };

    conn.execute(
        "INSERT INTO sessions (
            session_id, started_at, ended_at, active_chains, cross_chain_threshold_pct, status
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(session_id) DO UPDATE SET
            started_at = excluded.started_at,
            ended_at = excluded.ended_at,
            active_chains = excluded.active_chains,
            cross_chain_threshold_pct = excluded.cross_chain_threshold_pct,
            status = excluded.status",
        params![
            LEGACY_SESSION_ID,
            started_at,
            ended_at,
            metadata.active_chains,
            0.0f64,
            status,
        ],
    )
    .wrap_err("upsert legacy session failed")?;

    conn.execute(
        "UPDATE opportunities
         SET session_id = ?1
         WHERE session_id IS NULL OR session_id = ''",
        [LEGACY_SESSION_ID],
    )
    .wrap_err("backfill opportunities legacy session failed")?;
    conn.execute(
        "UPDATE price_snapshots
         SET session_id = ?1
         WHERE session_id IS NULL OR session_id = ''",
        [LEGACY_SESSION_ID],
    )
    .wrap_err("backfill price_snapshots legacy session failed")?;
    Ok(())
}

fn legacy_backfill_needed(conn: &Connection) -> eyre::Result<bool> {
    let missing_opportunities = count_missing_session_ids(conn, "opportunities")?;
    let missing_price_snapshots = count_missing_session_ids(conn, "price_snapshots")?;
    Ok(missing_opportunities > 0 || missing_price_snapshots > 0)
}

fn count_missing_session_ids(conn: &Connection, table: &str) -> eyre::Result<i64> {
    let query =
        format!("SELECT COUNT(*) FROM {table} WHERE session_id IS NULL OR TRIM(session_id) = ''");
    conn.query_row(&query, [], |row| row.get::<_, i64>(0))
        .wrap_err_with(|| format!("count missing session ids failed for {table}"))
}

fn backfill_session_status(conn: &Connection) -> eyre::Result<()> {
    conn.execute(
        "UPDATE sessions
         SET status = CASE
             WHEN ended_at IS NULL THEN ?1
             ELSE ?2
         END
         WHERE status IS NULL
            OR TRIM(status) = ''
            OR status NOT IN (?1, ?2, ?3)",
        params![
            SESSION_STATUS_ACTIVE,
            SESSION_STATUS_COMPLETED,
            SESSION_STATUS_RECOVERED,
        ],
    )
    .wrap_err("backfill session status failed")?;
    Ok(())
}

fn purge_orphaned_legacy_session(conn: &Connection) -> eyre::Result<()> {
    let legacy_opportunities = count_rows_for_session(conn, "opportunities", LEGACY_SESSION_ID)?;
    let legacy_price_snapshots =
        count_rows_for_session(conn, "price_snapshots", LEGACY_SESSION_ID)?;
    if legacy_opportunities == 0 && legacy_price_snapshots == 0 {
        conn.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            [LEGACY_SESSION_ID],
        )
        .wrap_err("delete orphaned legacy session failed")?;
    }
    Ok(())
}

fn count_rows_for_session(conn: &Connection, table: &str, session_id: &str) -> eyre::Result<i64> {
    let query = format!("SELECT COUNT(*) FROM {table} WHERE session_id = ?1");
    conn.query_row(&query, [session_id], |row| row.get::<_, i64>(0))
        .wrap_err_with(|| format!("count rows for session failed in {table}"))
}

struct LegacySessionMetadata {
    started_at: u64,
    ended_at: Option<u64>,
    active_chains: String,
}

fn legacy_session_metadata(conn: &Connection) -> eyre::Result<LegacySessionMetadata> {
    let opp_range = min_max_timestamp(conn, "opportunities")?;
    let snap_range = min_max_timestamp(conn, "price_snapshots")?;
    let started_at = [
        opp_range.map(|(min, _)| min),
        snap_range.map(|(min, _)| min),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(0);
    let ended_at = [
        opp_range.map(|(_, max)| max),
        snap_range.map(|(_, max)| max),
    ]
    .into_iter()
    .flatten()
    .max();

    let mut chains = BTreeSet::new();
    chains.extend(distinct_chains(conn, "opportunities")?);
    chains.extend(distinct_chains(conn, "price_snapshots")?);
    let active_chains = if chains.is_empty() {
        "legacy".to_string()
    } else {
        chains.into_iter().collect::<Vec<_>>().join(",")
    };

    Ok(LegacySessionMetadata {
        started_at,
        ended_at,
        active_chains,
    })
}

fn min_max_timestamp(conn: &Connection, table: &str) -> eyre::Result<Option<(u64, u64)>> {
    let query = format!("SELECT MIN(timestamp), MAX(timestamp) FROM {table}");
    let row = conn
        .query_row(&query, [], |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .wrap_err_with(|| format!("min/max timestamp query failed for {table}"))?;
    match row {
        (Some(min), Some(max)) => Ok(Some((
            checked_i64_to_u64(min, "timestamp min")?,
            checked_i64_to_u64(max, "timestamp max")?,
        ))),
        _ => Ok(None),
    }
}

fn distinct_chains(conn: &Connection, table: &str) -> eyre::Result<Vec<String>> {
    let query =
        format!("SELECT DISTINCT chain FROM {table} WHERE chain IS NOT NULL ORDER BY chain");
    let mut stmt = conn
        .prepare(&query)
        .wrap_err_with(|| format!("prepare distinct chains failed for {table}"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .wrap_err_with(|| format!("query distinct chains failed for {table}"))?
        .collect::<Result<Vec<_>, _>>()
        .wrap_err_with(|| format!("collect distinct chains failed for {table}"))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArbOpportunity, ChainId};
    use ethers::types::U256;

    fn make_session(id: &str, started_at: u64) -> SessionInfo {
        SessionInfo {
            id: id.to_string(),
            started_at,
        }
    }

    fn make_opp(path: &'static str, roi: f64, timestamp: u64) -> ArbOpportunity {
        ArbOpportunity {
            path,
            chain: ChainId::Ethereum,
            input_weth: U256::from(1_000_000_000_000_000_000u64),
            estimated_net_after_gas: 1000,
            roi_pct: roi,
            gas_cost_usd: 5.0,
            timestamp,
        }
    }

    #[test]
    fn test_create_session_and_latest_session_id() {
        let db = OpportunityDb::open(":memory:").unwrap();
        let first = make_session("session-1", 10);
        let second = make_session("session-2", 20);

        db.create_session(&first, "ethereum", 0.1).unwrap();
        db.create_session(&second, "ethereum,arbitrum", 0.2)
            .unwrap();

        assert_eq!(
            db.latest_session_id().unwrap(),
            Some("session-2".to_string())
        );

        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "session-2");
        assert_eq!(sessions[0].active_chains, "ethereum,arbitrum");
        assert_eq!(sessions[0].status, "active");

        db.close_session("session-2", 25).unwrap();
        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions[0].ended_at, Some(25));
        assert_eq!(sessions[0].status, "completed");
    }

    #[test]
    fn test_open_empty_db_does_not_create_legacy_session() {
        let db = OpportunityDb::open(":memory:").unwrap();

        let sessions = db.list_sessions().unwrap();

        assert!(sessions.is_empty());
    }

    #[test]
    fn test_recover_incomplete_sessions_marks_active_rows_recovered() {
        let db = OpportunityDb::open(":memory:").unwrap();
        let recovered = make_session("session-recovered", 10);
        let completed = make_session("session-completed", 20);
        db.create_session(&recovered, "ethereum", 0.1).unwrap();
        db.create_session(&completed, "base", 0.1).unwrap();
        db.close_session(&completed.id, 25).unwrap();

        let recovered_count = db.recover_incomplete_sessions(30).unwrap();

        assert_eq!(recovered_count, 1);
        let sessions = db.list_sessions().unwrap();
        let recovered_session = sessions
            .iter()
            .find(|session| session.session_id == recovered.id)
            .unwrap();
        assert_eq!(recovered_session.ended_at, Some(30));
        assert_eq!(recovered_session.status, "recovered");
        let completed_session = sessions
            .iter()
            .find(|session| session.session_id == completed.id)
            .unwrap();
        assert_eq!(completed_session.ended_at, Some(25));
        assert_eq!(completed_session.status, "completed");
    }

    #[test]
    fn test_reopening_session_aware_db_does_not_create_legacy_session() {
        let path = std::env::temp_dir().join("arb_session_aware_reopen.db");
        let _ = std::fs::remove_file(&path);

        let db = OpportunityDb::open(path.to_str().unwrap()).unwrap();
        let session = make_session("session-1", 1_700_000_000);
        db.create_session(&session, "ethereum", 0.1).unwrap();
        db.insert_price_snapshot(&session.id, "ethereum", 21_000_001, 3200.0)
            .unwrap();
        drop(db);

        let reopened = OpportunityDb::open(path.to_str().unwrap()).unwrap();
        let sessions = reopened.list_sessions().unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-1");
    }

    #[test]
    fn test_insert_and_query_opportunity() {
        let db = OpportunityDb::open(":memory:").unwrap();
        let session = make_session("session-1", 1_700_000_000);
        db.create_session(&session, "ethereum", 0.1).unwrap();
        db.insert_opportunity(&session.id, &make_opp("A→B→C", 0.01, session.started_at))
            .unwrap();
        let count = {
            let conn = db.conn.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM opportunities", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
        };
        assert_eq!(count, 1);
    }

    #[test]
    fn test_session_scoped_path_queries() {
        let db = OpportunityDb::open(":memory:").unwrap();
        let session_a = make_session("session-a", 1_700_000_000);
        let session_b = make_session("session-b", 1_700_000_100);
        db.create_session(&session_a, "ethereum", 0.1).unwrap();
        db.create_session(&session_b, "arbitrum", 0.1).unwrap();

        db.insert_opportunity(
            &session_a.id,
            &make_opp("PATH_A", 0.01, session_a.started_at),
        )
        .unwrap();
        db.insert_opportunity(
            &session_a.id,
            &make_opp("PATH_A", 0.02, session_a.started_at + 1),
        )
        .unwrap();
        db.insert_opportunity(
            &session_b.id,
            &make_opp("PATH_B", 0.50, session_b.started_at),
        )
        .unwrap();

        let top_a = db.top_paths_by_count(Some(&session_a.id), 5).unwrap();
        assert_eq!(top_a, vec![("PATH_A".to_string(), 2)]);

        let top_b = db.top_paths_by_avg_roi(Some(&session_b.id), 5).unwrap();
        assert_eq!(top_b, vec![("PATH_B".to_string(), 0.50)]);
    }

    #[test]
    fn test_export_csv_filters_by_session() {
        let db = OpportunityDb::open(":memory:").unwrap();
        let session_a = make_session("session-a", 1_700_000_000);
        let session_b = make_session("session-b", 1_700_000_100);
        db.create_session(&session_a, "ethereum", 0.1).unwrap();
        db.create_session(&session_b, "base", 0.1).unwrap();

        db.insert_opportunity(
            &session_a.id,
            &make_opp("A→B→C", 0.01, session_a.started_at),
        )
        .unwrap();
        db.insert_opportunity(
            &session_b.id,
            &make_opp("B→C→D", 0.02, session_b.started_at),
        )
        .unwrap();

        let path = std::env::temp_dir().join("test_arb_export_session.csv");
        db.export_csv(Some(&session_a.id), path.to_str().unwrap())
            .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("session-a"));
        assert!(!content.contains("session-b"));
    }

    #[test]
    fn test_migrates_legacy_schema_and_backfills_session_ids() {
        let path = std::env::temp_dir().join("arb_legacy_schema.db");
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE opportunities (
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
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO opportunities (timestamp, chain, path, input_eth, roi_pct, net_wei, gas_cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![1_700_000_000i64, "ethereum", "LEGACY_PATH", 1.0f64, 0.01f64, 1000i64, 5.0f64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO price_snapshots (timestamp, chain, block, weth_usd)
             VALUES (?1, ?2, ?3, ?4)",
            params![1_700_000_000i64, "ethereum", 21_000_000i64, 3200.0f64],
        )
        .unwrap();
        drop(conn);

        let db = OpportunityDb::open(path.to_str().unwrap()).unwrap();
        let conn = db.conn.lock().unwrap();
        let session_id: String = conn
            .query_row("SELECT session_id FROM opportunities LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(session_id, LEGACY_SESSION_ID);
        let legacy_session = conn
            .query_row(
                "SELECT session_id, active_chains, status FROM sessions WHERE session_id = ?1",
                [LEGACY_SESSION_ID],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(legacy_session.0, LEGACY_SESSION_ID);
        assert_eq!(legacy_session.1, "ethereum");
        assert_eq!(legacy_session.2, "completed");
    }

    #[test]
    fn test_checked_i128_to_i64_rejects_overflow() {
        let err = checked_i128_to_i64(i64::MAX as i128 + 1, "net_wei").unwrap_err();
        assert!(err.to_string().contains("net_wei"));
    }

    #[test]
    fn test_insert_opportunity_rejects_net_wei_out_of_i64_range() {
        let db = OpportunityDb::open(":memory:").unwrap();
        let session = make_session("session-1", 1_700_000_000);
        db.create_session(&session, "ethereum", 0.1).unwrap();
        let mut opp = make_opp("A→B→C", 0.01, session.started_at);
        opp.estimated_net_after_gas = i64::MAX as i128 + 1;

        let err = db.insert_opportunity(&session.id, &opp).unwrap_err();

        assert!(err.to_string().contains("net_wei"));
    }
}
