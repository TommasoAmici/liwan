use crate::config::DuckdbConfig;
use crate::utils::refinery_duckdb::DuckDBConnection;
use crate::utils::refinery_sqlite::RqlConnection;

use crate::utils::r2d2_sqlite::SqliteConnectionManager;
use anyhow::{Context, Result, bail};
use duckdb::DuckdbConnectionManager;
use refinery::Runner;
use std::path::PathBuf;

pub(super) fn init_duckdb(
    path: &PathBuf,
    duckdb_config: DuckdbConfig,
    mut migrations_runner: Runner,
) -> Result<r2d2::Pool<DuckdbConnectionManager>> {
    let mut tries = 10;
    let conn = loop {
        let mut flags = duckdb::Config::default()
            .enable_autoload_extension(true)?
            .access_mode(duckdb::AccessMode::ReadWrite)?
            .with("enable_fsst_vectors", "true")?
            .with("allocator_background_threads", "true")?;

        if let Some(memory_limit) = &duckdb_config.memory_limit {
            flags = flags.max_memory(memory_limit)?;
        }

        if let Some(threads) = duckdb_config.threads {
            flags = flags.threads(threads.get().into())?;
        }

        match DuckdbConnectionManager::file_with_flags(path, flags) {
            Ok(conn) => break conn,
            Err(e) => {
                if tries <= 0 {
                    tracing::warn!("");
                    return Err(e).context("Failed to load DuckDB Database after 10 attempts");
                }

                if e.to_string().contains("Could not set lock on file") {
                    tracing::warn!("DuckDB database is locked. Retrying... ({} tries left)", tries);
                    tries -= 1;
                    std::thread::sleep(std::time::Duration::from_secs(1));
                } else {
                    return Err(e).context("Failed to load DuckDB Database");
                }
            }
        }
    };

    let pool = r2d2::Pool::new(conn)?;
    {
        let conn = pool.get()?;
        conn.execute("PRAGMA enable_checkpoint_on_shutdown", [])?;
        conn.pragma_update(None, "autoload_known_extensions", &"true")?;
        conn.pragma_update(None, "allow_community_extensions", &"false")?;
    }

    {
        let conn = pool.get()?;
        migrations_runner.set_migration_table_name("migrations");
        for migration in migrations_runner.run_iter(&mut DuckDBConnection(conn)) {
            match migration {
                Ok(migration) => {
                    tracing::info!("Applied migration: {}", migration);
                }
                Err(err) => {
                    bail!("Failed to apply migration: {}", err);
                }
            }
        }
    }

    warn_on_outdated_storage_version(&pool);

    Ok(pool)
}

/// Parse the major version out of a DuckDB `storage_version` tag such as
/// `v2.0.0+`. Returns `None` for anything that does not start with `v<digits>`.
fn storage_major(version: &str) -> Option<u32> {
    let digits: String = version.strip_prefix('v')?.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Tell the operator once when the event database still uses the pre-2.0
/// storage format, which keeps working with the bundled engine.
///
/// Reading the tag must never stop startup, so every failure is logged at
/// debug level: the conversion is an explicit `liwan upgrade-duckdb` run, not
/// something Liwan does on its own.
fn warn_on_outdated_storage_version(pool: &r2d2::Pool<DuckdbConnectionManager>) {
    let read = || -> Result<String> {
        let conn = pool.get()?;
        Ok(conn.query_row(
            "SELECT tags['storage_version'] FROM duckdb_databases() \
             WHERE database_name NOT IN ('system', 'temp')",
            [],
            |row| row.get(0),
        )?)
    };

    let version = match read() {
        Ok(version) => version,
        Err(err) => {
            tracing::debug!(error = %err, "Could not read the DuckDB storage version");
            return;
        }
    };

    match storage_major(&version) {
        Some(major) if major < 2 => tracing::warn!(
            storage_version = %version,
            "The event database still uses the old DuckDB storage format; run `liwan upgrade-duckdb` \
             with Liwan stopped to convert it. The original file is kept as .old"
        ),
        Some(_) => {}
        None => tracing::debug!(storage_version = %version, "Unrecognized DuckDB storage version"),
    }
}

pub fn init_duckdb_mem(mut migrations_runner: Runner) -> Result<r2d2::Pool<DuckdbConnectionManager>> {
    let conn = DuckdbConnectionManager::memory()?;
    let pool = r2d2::Pool::new(conn)?;
    migrations_runner.set_migration_table_name("migrations");
    migrations_runner.run(&mut DuckDBConnection(pool.get()?))?;

    {
        let conn = pool.get()?;
        conn.pragma_update(None, "allow_community_extensions", &"false")?;
        conn.pragma_update(None, "enable_fsst_vectors", &"true")?;
    }

    Ok(pool)
}

pub(super) fn init_sqlite(
    path: &PathBuf,
    mut migrations_runner: Runner,
) -> Result<r2d2::Pool<SqliteConnectionManager>> {
    let conn = SqliteConnectionManager::file(path);
    let pool = r2d2::Pool::new(conn)?;
    migrations_runner.set_migration_table_name("migrations");
    migrations_runner.run(&mut RqlConnection(pool.get()?))?;

    {
        let conn = pool.get()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "mmap_size", "268435456")?;
        conn.pragma_update(None, "journal_size_limit", "268435456")?;
        conn.pragma_update(None, "cache_size", "2000")?;
    }

    Ok(pool)
}

pub fn init_sqlite_mem(mut migrations_runner: Runner) -> Result<r2d2::Pool<SqliteConnectionManager>> {
    let conn = SqliteConnectionManager::memory();
    let pool = r2d2::Pool::new(conn)?;
    migrations_runner.set_migration_table_name("migrations");
    migrations_runner.run(&mut RqlConnection(pool.get()?))?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::storage_major;

    #[test]
    fn storage_major_reads_the_leading_version() {
        assert_eq!(storage_major("v1.5.0+"), Some(1));
        assert_eq!(storage_major("v2.0.0+"), Some(2));
        assert_eq!(storage_major("v2.1.0+"), Some(2));
        assert_eq!(storage_major("v3.0.0+"), Some(3));
    }

    #[test]
    fn storage_major_rejects_malformed_versions() {
        for version in ["", "2.0.0+", "v", "vx.y.z", " v2.0.0+", "v-2.0.0"] {
            assert_eq!(storage_major(version), None, "{version:?}");
        }
    }

    #[test]
    fn fresh_database_is_written_in_the_v2_storage_format() {
        let dir = tempfile::tempdir().unwrap();
        let pool = super::init_duckdb(
            &dir.path().join("liwan-events.duckdb"),
            crate::config::DuckdbConfig::default(),
            super::super::embedded::events::migrations::runner(),
        )
        .unwrap();

        let version: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT tags['storage_version'] FROM duckdb_databases() WHERE database_name NOT IN ('system', 'temp')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(version.starts_with("v2."), "fresh database uses {version}");
    }

    #[test]
    fn app_migrations_create_the_entity_index() {
        let dir = tempfile::tempdir().unwrap();
        let pool =
            super::init_sqlite(&dir.path().join("liwan-app.sqlite"), super::super::embedded::app::migrations::runner())
                .unwrap();

        let exists: bool = pool
            .get()
            .unwrap()
            .query_row(
                "select exists(select 1 from sqlite_master where type = 'index' and name = 'project_entities_entity_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "the entity index migration did not apply");
    }

    #[test]
    fn project_order_migration_preserves_the_existing_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "create table projects (id text primary key not null, display_name text not null);
             insert into projects (id, display_name) values ('a', 'Alpha'), ('b', 'Beta'), ('c', 'Gamma');",
        )
        .unwrap();

        conn.execute_batch(include_str!("../migrations/app/V12__project_order.sql")).unwrap();

        let order = conn
            .prepare("select id from projects order by sort_order")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(order, ["a", "b", "c"]);
    }
}
