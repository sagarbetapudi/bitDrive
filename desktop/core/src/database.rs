//! Database layer for BPL Desktop

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, error, info, warn};

use bpl_protocol::{DeviceId, SessionId, ProtocolError, Result};

/// Database connection pool type
pub type DatabasePool = Pool<SqliteConnectionManager>;

/// Pooled connection wrapper
pub struct PooledConnection(r2d2::PooledConnection<SqliteConnectionManager>);

impl std::ops::Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Database wrapper
pub struct Database {
    pool: DatabasePool,
    path: String,
}

impl Database {
    /// Create new database
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Ensure parent directory exists
        if let Some(parent) = path.as_ref().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let manager = SqliteConnectionManager::file(path.as_ref())
            .with_init(|conn| {
                // Configure SQLite
                conn.execute("PRAGMA journal_mode = WAL", [])?;
                conn.execute("PRAGMA synchronous = NORMAL", [])?;
                conn.execute("PRAGMA busy_timeout = 5000", [])?;
                conn.execute("PRAGMA foreign_keys = ON", [])?;
                conn.execute("PRAGMA temp_store = MEMORY", [])?;
                conn.execute("PRAGMA page_size = 4096", [])?;
                conn.execute("PRAGMA cache_size = -32768", [])?; // 32MB cache
                Ok(())
            });

        let pool = Pool::builder()
            .max_size(10)
            .min_idle(Some(2))
            .connection_timeout(Duration::from_secs(10))
            .build(manager)
            .map_err(|e| ProtocolError::Database(e.to_string()))?;

        let db = Self {
            pool,
            path: path_str,
        };

        // Run migrations
        db.migrate().await?;

        info!("Database initialized at {}", db.path);
        Ok(db)
    }

    /// Get a connection from the pool
    pub fn get(&self) -> Result<PooledConnection> {
        self.pool.get()
            .map_err(|e| ProtocolError::Database(e.to_string()))
            .map(PooledConnection)
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        let conn = self.get()?;
        Self::run_migrations(&conn)
    }

    /// Run all migrations
    fn run_migrations(conn: &PooledConnection) -> Result<()> {
        let migrations = [
            Self::migration_001_initial_schema,
            Self::migration_002_add_indexes,
            Self::migration_003_add_config_table,
            Self::migration_004_add_sync_tables,
            Self::migration_005_add_photo_backup,
            Self::migration_006_add_shell_history,
            Self::migration_007_add_config,
        ];

        // Create migrations table if not exists
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            )",
            [],
        )?;

        for (i, migration) in migrations.iter().enumerate() {
            let version = (i + 1) as i64;

            let applied: bool = conn.query_row(
                "SELECT 1 FROM schema_migrations WHERE version = ?",
                params![version],
                |_| Ok(true),
            ).optional()?.unwrap_or(false);

            if !applied {
                info!("Applying migration {}", version);
                migration(&conn)?;

                conn.execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)",
                    params![version, Self::current_timestamp()],
                )?;
            }
        }

        Ok(())
    }

    /// Migration 1: Initial schema
    fn migration_001_initial_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(r#"
            -- Devices table
            CREATE TABLE IF NOT EXISTS devices (
                id BLOB PRIMARY KEY,
                name TEXT,
                address TEXT NOT NULL UNIQUE,
                paired INTEGER NOT NULL DEFAULT 0,
                trusted INTEGER NOT NULL DEFAULT 0,
                psk BLOB,
                last_seen INTEGER,
                created_at INTEGER NOT NULL,
                metadata TEXT
            );

            -- Sessions table
            CREATE TABLE IF NOT EXISTS sessions (
                id BLOB PRIMARY KEY,
                device_id BLOB NOT NULL,
                protocol_version TEXT NOT NULL,
                session_keys BLOB,
                capabilities TEXT,
                state TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_activity INTEGER NOT NULL,
                bytes_sent INTEGER DEFAULT 0,
                bytes_received INTEGER DEFAULT 0,
                FOREIGN KEY (device_id) REFERENCES devices (id) ON DELETE CASCADE
            );

            -- Services table
            CREATE TABLE IF NOT EXISTS services (
                id TEXT PRIMARY KEY,
                session_id BLOB NOT NULL,
                name TEXT NOT NULL,
                version INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                channel_type TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 0,
                healthy INTEGER NOT NULL DEFAULT 1,
                registered_at INTEGER NOT NULL,
                last_heartbeat INTEGER,
                metadata TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE CASCADE
            );

            -- Sync jobs table
            CREATE TABLE IF NOT EXISTS sync_jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                direction TEXT NOT NULL,
                local_path TEXT NOT NULL,
                remote_path TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                auto_sync INTEGER NOT NULL DEFAULT 0,
                schedule_type TEXT,
                schedule_value TEXT,
                conflict_strategy TEXT NOT NULL DEFAULT 'last_write_wins',
                filters TEXT,
                status TEXT NOT NULL DEFAULT 'idle',
                last_sync INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                stats TEXT
            );

            -- Sync conflicts table
            CREATE TABLE IF NOT EXISTS sync_conflicts (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                local_path TEXT NOT NULL,
                remote_path TEXT NOT NULL,
                local_metadata TEXT,
                remote_metadata TEXT,
                local_hash TEXT,
                remote_hash TEXT,
                detected_at INTEGER NOT NULL,
                resolved INTEGER NOT NULL DEFAULT 0,
                resolution_strategy TEXT,
                FOREIGN KEY (job_id) REFERENCES sync_jobs (id) ON DELETE CASCADE
            );

            -- Photo backup sessions
            CREATE TABLE IF NOT EXISTS photo_backup_sessions (
                id TEXT PRIMARY KEY,
                device_id BLOB NOT NULL,
                status TEXT NOT NULL,
                total_photos INTEGER DEFAULT 0,
                backed_up INTEGER DEFAULT 0,
                skipped INTEGER DEFAULT 0,
                errors INTEGER DEFAULT 0,
                bytes_total INTEGER DEFAULT 0,
                bytes_transferred INTEGER DEFAULT 0,
                started_at INTEGER NOT NULL,
                completed_at INTEGER,
                config TEXT
            );

            -- Photos table
            CREATE TABLE IF NOT EXISTS photos (
                id TEXT PRIMARY KEY,
                backup_session_id TEXT,
                filename TEXT NOT NULL,
                path TEXT NOT NULL,
                size INTEGER NOT NULL,
                mime_type TEXT,
                width INTEGER,
                height INTEGER,
                created_time INTEGER NOT NULL,
                modified_time INTEGER NOT NULL,
                hash TEXT,
                exif TEXT,
                location_lat REAL,
                location_lon REAL,
                albums TEXT,
                favorite INTEGER DEFAULT 0,
                trashed INTEGER DEFAULT 0,
                FOREIGN KEY (backup_session_id) REFERENCES photo_backup_sessions (id)
            );

            -- Shell sessions
            CREATE TABLE IF NOT EXISTS shell_sessions (
                id TEXT PRIMARY KEY,
                device_id BLOB NOT NULL,
                name TEXT,
                working_directory TEXT,
                environment TEXT,
                state TEXT NOT NULL,
                cols INTEGER,
                rows INTEGER,
                created_at INTEGER NOT NULL,
                last_activity INTEGER NOT NULL,
                FOREIGN KEY (device_id) REFERENCES devices (id) ON DELETE CASCADE
            );

            -- Shell history
            CREATE TABLE IF NOT EXISTS shell_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                command TEXT NOT NULL,
                exit_code INTEGER,
                executed_at INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES shell_sessions (id) ON DELETE CASCADE
            );

            -- Config table
            CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                description TEXT,
                read_only INTEGER DEFAULT 0,
                secret INTEGER DEFAULT 0,
                updated_at INTEGER NOT NULL
            );
        "#)?;

        Ok(())
    }

    /// Migration 2: Add indexes
    fn migration_002_add_indexes(conn: &Connection) -> Result<()> {
        conn.execute_batch(r#"
            CREATE INDEX IF NOT EXISTS idx_devices_address ON devices (address);
            CREATE INDEX IF NOT EXISTS idx_devices_paired ON devices (paired);
            CREATE INDEX IF NOT EXISTS idx_devices_trusted ON devices (trusted);
            CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions (device_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_state ON sessions (state);
            CREATE INDEX IF NOT EXISTS idx_services_session ON services (session_id);
            CREATE INDEX IF NOT EXISTS idx_sync_jobs_enabled ON sync_jobs (enabled);
            CREATE INDEX IF NOT EXISTS idx_sync_conflicts_job ON sync_conflicts (job_id);
            CREATE INDEX IF NOT EXISTS idx_photos_session ON photos (backup_session_id);
            CREATE INDEX IF NOT EXISTS idx_photos_hash ON photos (hash);
            CREATE INDEX IF NOT EXISTS idx_shell_sessions_device ON shell_sessions (device_id);
            CREATE INDEX IF NOT EXISTS idx_shell_history_session ON shell_history (session_id);
        "#)?;

        Ok(())
    }

    /// Migration 3: Add config table (already in migration 1, but ensure it exists)
    fn migration_003_add_config_table(conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                description TEXT,
                read_only INTEGER DEFAULT 0,
                secret INTEGER DEFAULT 0,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    /// Migration 4: Add sync tables (already in migration 1)
    fn migration_004_add_sync_tables(conn: &Connection) -> Result<()> {
        // Tables already created in migration 1
        Ok(())
    }

    /// Migration 5: Add photo backup tables (already in migration 1)
    fn migration_005_add_photo_backup(conn: &Connection) -> Result<()> {
        // Tables already created in migration 1
        Ok(())
    }

    /// Migration 6: Add shell history table (already in migration 1)
    fn migration_006_add_shell_history(conn: &Connection) -> Result<()> {
        // Table already created in migration 1
        Ok(())
    }

    /// Migration 7: Add additional config columns
    fn migration_007_add_config(conn: &Connection) -> Result<()> {
        conn.execute(
            "ALTER TABLE config ADD COLUMN secret INTEGER DEFAULT 0",
            [],
        ).or_else(|_| Ok(()))?; // Ignore if column exists
        Ok(())
    }

    /// Get current timestamp in milliseconds
    fn current_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    /// Device operations
    pub fn upsert_device(&self, device_id: &DeviceId, name: Option<&str>, address: &str, paired: bool, trusted: bool, psk: Option<&[u8]>) -> Result<()> {
        let conn = self.get()?;
        let now = Self::current_timestamp();

        conn.execute(
            r#"
            INSERT INTO devices (id, name, address, paired, trusted, psk, last_seen, created_at, metadata)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, '{}')
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                address = excluded.address,
                paired = excluded.paired,
                trusted = excluded.trusted,
                psk = excluded.psk,
                last_seen = excluded.last_seen
            "#,
            params![
                &device_id.value,
                name,
                address,
                paired as i64,
                trusted as i64,
                psk,
                now,
                now,
            ],
        )?;

        Ok(())
    }

    pub fn get_device(&self, device_id: &DeviceId) -> Result<Option<DeviceRecord>> {
        let conn = self.get()?;

        let record = conn.query_row(
            "SELECT id, name, address, paired, trusted, psk, last_seen, created_at, metadata FROM devices WHERE id = ?",
            params![&device_id.value],
            |row| Ok(DeviceRecord {
                id: DeviceId { value: row.get(0)? },
                name: row.get(1)?,
                address: row.get(2)?,
                paired: row.get(3)?,
                trusted: row.get(4)?,
                psk: row.get(5)?,
                last_seen: row.get(6)?,
                created_at: row.get(7)?,
                metadata: row.get(8)?,
            }),
        ).optional()?;

        Ok(record)
    }

    pub fn get_paired_devices(&self) -> Result<Vec<DeviceRecord>> {
        let conn = self.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, name, address, paired, trusted, psk, last_seen, created_at, metadata FROM devices WHERE paired = 1"
        )?;

        let records = stmt.query_map([], |row| Ok(DeviceRecord {
            id: DeviceId { value: row.get(0)? },
            name: row.get(1)?,
            address: row.get(2)?,
            paired: row.get(3)?,
            trusted: row.get(4)?,
            psk: row.get(5)?,
            last_seen: row.get(6)?,
            created_at: row.get(7)?,
            metadata: row.get(8)?,
        }))?;

        let mut result = Vec::new();
        for record in records {
            result.push(record?);
        }

        Ok(result)
    }

    /// Session operations
    pub fn create_session(&self, session_id: &SessionId, device_id: &DeviceId, protocol_version: &str, session_keys: &[u8], capabilities: &str) -> Result<()> {
        let conn = self.get()?;
        let now = Self::current_timestamp();

        conn.execute(
            r#"
            INSERT INTO sessions (id, device_id, protocol_version, session_keys, capabilities, state, created_at, last_activity)
            VALUES (?, ?, ?, ?, ?, 'opening', ?, ?)
            "#,
            params![
                &session_id.value,
                &device_id.value,
                protocol_version,
                session_keys,
                capabilities,
                now,
                now,
            ],
        )?;

        Ok(())
    }

    pub fn update_session_state(&self, session_id: &SessionId, state: &str) -> Result<()> {
        let conn = self.get()?;
        let now = Self::current_timestamp();

        conn.execute(
            "UPDATE sessions SET state = ?, last_activity = ? WHERE id = ?",
            params![state, now, &session_id.value],
        )?;

        Ok(())
    }

    pub fn get_session(&self, session_id: &SessionId) -> Result<Option<SessionRecord>> {
        let conn = self.get()?;

        let record = conn.query_row(
            "SELECT id, device_id, protocol_version, session_keys, capabilities, state, created_at, last_activity, bytes_sent, bytes_received FROM sessions WHERE id = ?",
            params![&session_id.value],
            |row| Ok(SessionRecord {
                id: SessionId { value: row.get(0)? },
                device_id: DeviceId { value: row.get(1)? },
                protocol_version: row.get(2)?,
                session_keys: row.get(3)?,
                capabilities: row.get(4)?,
                state: row.get(5)?,
                created_at: row.get(6)?,
                last_activity: row.get(7)?,
                bytes_sent: row.get(8)?,
                bytes_received: row.get(9)?,
            }),
        ).optional()?;

        Ok(record)
    }

    pub fn get_active_sessions(&self) -> Result<Vec<SessionRecord>> {
        let conn = self.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, device_id, protocol_version, session_keys, capabilities, state, created_at, last_activity, bytes_sent, bytes_received FROM sessions WHERE state IN ('opening', 'negotiating', 'authenticating', 'active')"
        )?;

        let records = stmt.query_map([], |row| Ok(SessionRecord {
            id: SessionId { value: row.get(0)? },
            device_id: DeviceId { value: row.get(1)? },
            protocol_version: row.get(2)?,
            session_keys: row.get(3)?,
            capabilities: row.get(4)?,
            state: row.get(5)?,
            created_at: row.get(6)?,
            last_activity: row.get(7)?,
            bytes_sent: row.get(8)?,
            bytes_received: row.get(9)?,
        }))?;

        let mut result = Vec::new();
        for record in records {
            result.push(record?);
        }

        Ok(result)
    }

    /// Config operations
    pub fn set_config(&self, key: &str, value: &str, description: Option<&str>, read_only: bool, secret: bool) -> Result<()> {
        let conn = self.get()?;
        let now = Self::current_timestamp();

        conn.execute(
            r#"
            INSERT INTO config (key, value, description, read_only, secret, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                description = excluded.description,
                read_only = excluded.read_only,
                secret = excluded.secret,
                updated_at = excluded.updated_at
            "#,
            params![key, value, description, read_only as i64, secret as i64, now],
        )?;

        Ok(())
    }

    pub fn get_config(&self, key: &str) -> Result<Option<String>> {
        let conn = self.get()?;

        let value = conn.query_row(
            "SELECT value FROM config WHERE key = ?",
            params![key],
            |row| row.get(0),
        ).optional()?;

        Ok(value)
    }

    pub fn delete_config(&self, key: &str) -> Result<()> {
        let conn = self.get()?;
        conn.execute("DELETE FROM config WHERE key = ? AND read_only = 0", params![key])?;
        Ok(())
    }

    pub fn list_config(&self) -> Result<Vec<ConfigRecord>> {
        let conn = self.get()?;

        let mut stmt = conn.prepare(
            "SELECT key, value, description, read_only, secret, updated_at FROM config"
        )?;

        let records = stmt.query_map([], |row| Ok(ConfigRecord {
            key: row.get(0)?,
            value: row.get(1)?,
            description: row.get(2)?,
            read_only: row.get(3)?,
            secret: row.get(4)?,
            updated_at: row.get(5)?,
        }))?;

        let mut result = Vec::new();
        for record in records {
            result.push(record?);
        }

        Ok(result)
    }

    /// Sync job operations
    pub fn create_sync_job(&self, job: &SyncJobRecord) -> Result<()> {
        let conn = self.get()?;

        conn.execute(
            r#"
            INSERT INTO sync_jobs (id, name, description, direction, local_path, remote_path, enabled, auto_sync, schedule_type, schedule_value, conflict_strategy, filters, status, last_sync, created_at, updated_at, stats)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'idle', ?, ?, ?, '{}')
            "#,
            params![
                &job.id,
                &job.name,
                &job.description,
                &job.direction,
                &job.local_path,
                &job.remote_path,
                job.enabled as i64,
                job.auto_sync as i64,
                &job.schedule_type,
                &job.schedule_value,
                &job.conflict_strategy,
                &job.filters,
                job.last_sync,
                job.created_at,
                job.updated_at,
            ],
        )?;

        Ok(())
    }

    pub fn get_sync_job(&self, job_id: &str) -> Result<Option<SyncJobRecord>> {
        let conn = self.get()?;

        let record = conn.query_row(
            "SELECT id, name, description, direction, local_path, remote_path, enabled, auto_sync, schedule_type, schedule_value, conflict_strategy, filters, status, last_sync, created_at, updated_at, stats FROM sync_jobs WHERE id = ?",
            params![job_id],
            |row| Ok(SyncJobRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                direction: row.get(3)?,
                local_path: row.get(4)?,
                remote_path: row.get(5)?,
                enabled: row.get(6)?,
                auto_sync: row.get(7)?,
                schedule_type: row.get(8)?,
                schedule_value: row.get(9)?,
                conflict_strategy: row.get(10)?,
                filters: row.get(11)?,
                status: row.get(12)?,
                last_sync: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
                stats: row.get(16)?,
            }),
        ).optional()?;

        Ok(record)
    }

    pub fn list_sync_jobs(&self) -> Result<Vec<SyncJobRecord>> {
        let conn = self.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, name, description, direction, local_path, remote_path, enabled, auto_sync, schedule_type, schedule_value, conflict_strategy, filters, status, last_sync, created_at, updated_at, stats FROM sync_jobs"
        )?;

        let records = stmt.query_map([], |row| Ok(SyncJobRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            direction: row.get(3)?,
            local_path: row.get(4)?,
            remote_path: row.get(5)?,
            enabled: row.get(6)?,
            auto_sync: row.get(7)?,
            schedule_type: row.get(8)?,
            schedule_value: row.get(9)?,
            conflict_strategy: row.get(10)?,
            filters: row.get(11)?,
            status: row.get(12)?,
            last_sync: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
            stats: row.get(16)?,
        }))?;

        let mut result = Vec::new();
        for record in records {
            result.push(record?);
        }

        Ok(result)
    }

    pub fn update_sync_job_status(&self, job_id: &str, status: &str) -> Result<()> {
        let conn = self.get()?;
        let now = Self::current_timestamp();

        conn.execute(
            "UPDATE sync_jobs SET status = ?, updated_at = ? WHERE id = ?",
            params![status, now, job_id],
        )?;

        Ok(())
    }

    pub fn update_sync_job_last_sync(&self, job_id: &str, last_sync: i64) -> Result<()> {
        let conn = self.get()?;
        let now = Self::current_timestamp();

        conn.execute(
            "UPDATE sync_jobs SET last_sync = ?, updated_at = ? WHERE id = ?",
            params![last_sync, now, job_id],
        )?;

        Ok(())
    }

    pub fn delete_sync_job(&self, job_id: &str) -> Result<()> {
        let conn = self.get()?;
        conn.execute("DELETE FROM sync_jobs WHERE id = ?", params![job_id])?;
        Ok(())
    }

    /// Sync conflict operations
    pub fn add_sync_conflict(&self, conflict: &SyncConflictRecord) -> Result<()> {
        let conn = self.get()?;

        conn.execute(
            r#"
            INSERT INTO sync_conflicts (id, job_id, local_path, remote_path, local_metadata, remote_metadata, local_hash, remote_hash, detected_at, resolved, resolution_strategy)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)
            "#,
            params![
                &conflict.id,
                &conflict.job_id,
                &conflict.local_path,
                &conflict.remote_path,
                &conflict.local_metadata,
                &conflict.remote_metadata,
                &conflict.local_hash,
                &conflict.remote_hash,
                conflict.detected_at,
                &conflict.resolution_strategy,
            ],
        )?;

        Ok(())
    }

    pub fn get_sync_conflicts(&self, job_id: &str) -> Result<Vec<SyncConflictRecord>> {
        let conn = self.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, job_id, local_path, remote_path, local_metadata, remote_metadata, local_hash, remote_hash, detected_at, resolved, resolution_strategy FROM sync_conflicts WHERE job_id = ?"
        )?;

        let records = stmt.query_map(params![job_id], |row| Ok(SyncConflictRecord {
            id: row.get(0)?,
            job_id: row.get(1)?,
            local_path: row.get(2)?,
            remote_path: row.get(3)?,
            local_metadata: row.get(4)?,
            remote_metadata: row.get(5)?,
            local_hash: row.get(6)?,
            remote_hash: row.get(7)?,
            detected_at: row.get(8)?,
            resolved: row.get(9)?,
            resolution_strategy: row.get(10)?,
        }))?;

        let mut result = Vec::new();
        for record in records {
            result.push(record?);
        }

        Ok(result)
    }

    pub fn resolve_sync_conflict(&self, conflict_id: &str, strategy: &str) -> Result<()> {
        let conn = self.get()?;

        conn.execute(
            "UPDATE sync_conflicts SET resolved = 1, resolution_strategy = ? WHERE id = ?",
            params![strategy, conflict_id],
        )?;

        Ok(())
    }
}

/// Device record
#[derive(Debug, Clone)]
pub struct DeviceRecord {
    pub id: DeviceId,
    pub name: Option<String>,
    pub address: String,
    pub paired: bool,
    pub trusted: bool,
    pub psk: Option<Vec<u8>>,
    pub last_seen: Option<i64>,
    pub created_at: i64,
    pub metadata: String,
}

/// Session record
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: SessionId,
    pub device_id: DeviceId,
    pub protocol_version: String,
    pub session_keys: Vec<u8>,
    pub capabilities: String,
    pub state: String,
    pub created_at: i64,
    pub last_activity: i64,
    pub bytes_sent: i64,
    pub bytes_received: i64,
}

/// Config record
#[derive(Debug, Clone)]
pub struct ConfigRecord {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
    pub read_only: bool,
    pub secret: bool,
    pub updated_at: i64,
}

/// Sync job record
#[derive(Debug, Clone)]
pub struct SyncJobRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub direction: String,
    pub local_path: String,
    pub remote_path: String,
    pub enabled: bool,
    pub auto_sync: bool,
    pub schedule_type: Option<String>,
    pub schedule_value: Option<String>,
    pub conflict_strategy: String,
    pub filters: String,
    pub status: String,
    pub last_sync: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub stats: String,
}

/// Sync conflict record
#[derive(Debug, Clone)]
pub struct SyncConflictRecord {
    pub id: String,
    pub job_id: String,
    pub local_path: String,
    pub remote_path: String,
    pub local_metadata: String,
    pub remote_metadata: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub detected_at: i64,
    pub resolved: bool,
    pub resolution_strategy: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_database_creation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        // Test device operations
        let device_id = bpl_protocol::DeviceId { value: vec![1, 2, 3, 4, 5, 6] };
        db.upsert_device(&device_id, Some("Test Device"), "AA:BB:CC:DD:EE:FF", true, true, None).unwrap();

        let device = db.get_device(&device_id).unwrap();
        assert!(device.is_some());
        assert_eq!(device.unwrap().name, Some("Test Device".to_string()));

        // Test config operations
        db.set_config("test.key", "test_value", Some("Test config"), false, false).unwrap();
        let value = db.get_config("test.key").unwrap();
        assert_eq!(value, Some("test_value".to_string()));

        // Test sync job
        let job = SyncJobRecord {
            id: "test-job".to_string(),
            name: "Test Job".to_string(),
            description: "Test".to_string(),
            direction: "bidirectional".to_string(),
            local_path: "/test".to_string(),
            remote_path: "/remote".to_string(),
            enabled: true,
            auto_sync: false,
            schedule_type: None,
            schedule_value: None,
            conflict_strategy: "last_write_wins".to_string(),
            filters: "".to_string(),
            status: "idle".to_string(),
            last_sync: None,
            created_at: 1000,
            updated_at: 1000,
            stats: "{}".to_string(),
        };

        db.create_sync_job(&job).unwrap();
        let retrieved = db.get_sync_job("test-job").unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Job");
    }
}