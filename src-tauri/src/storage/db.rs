//! SQLite persistence for server profiles.
//! Schema matches Android `ProfileEntity`, plus station metadata columns.

use std::path::Path;
use rusqlite::{params, Connection, Result, Row};
use serde::{Deserialize, Serialize};

/// Stored in the password columns when the real secret lives in Credential Manager.
pub const KEYRING_SENTINEL: &str = "credential_manager";

fn default_sid() -> u32 { 1 }
fn default_codec() -> String { "MP3".to_string() }
fn default_bitrate() -> u32 { 64 }
fn default_sample_rate() -> u32 { 44100 }
fn default_mode() -> String { "SOURCE".to_string() }
fn default_dj_protocol() -> String { "ULTRAVOX".to_string() }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRecord {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(alias = "server")]
    pub host: String,
    pub port: u16,
    #[serde(default = "default_sid")]
    pub sid: u32,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub mount: String,
    #[serde(default = "default_codec")]
    pub codec: String,
    #[serde(alias = "bitrate", default = "default_bitrate")]
    pub bitrate_kbps: u32,
    #[serde(alias = "samplerate", default = "default_sample_rate")]
    pub sample_rate_hz: u32,
    #[serde(default)]
    pub mono: bool,
    #[serde(default)]
    pub admin_user: Option<String>,
    #[serde(default)]
    pub admin_password: Option<String>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(alias = "username", default)]
    pub dj_user: Option<String>,
    #[serde(default)]
    pub dj_password: Option<String>,
    #[serde(default)]
    pub dj_port: Option<u16>,
    #[serde(alias = "protocol", default = "default_dj_protocol")]
    pub dj_protocol: String,
    #[serde(default)]
    pub dj_mount: Option<String>,
    /// Station name announced to listeners (blank -> profile name).
    #[serde(default)]
    pub stream_name: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub station_url: String,
    #[serde(default = "default_true")]
    pub is_public: bool,

    // UI-only flags (never stored): passwords are not sent to the webview,
    // only whether one is set.
    #[serde(default)]
    pub has_password: bool,
    #[serde(default)]
    pub has_dj_password: bool,
}

impl Default for ProfileRecord {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            host: String::new(),
            port: 8000,
            sid: default_sid(),
            password: String::new(),
            mount: String::new(),
            codec: default_codec(),
            bitrate_kbps: default_bitrate(),
            sample_rate_hz: default_sample_rate(),
            mono: true,
            admin_user: None,
            admin_password: None,
            mode: default_mode(),
            dj_user: None,
            dj_password: None,
            dj_port: None,
            dj_protocol: default_dj_protocol(),
            dj_mount: None,
            stream_name: String::new(),
            genre: String::new(),
            station_url: String::new(),
            is_public: true,
            has_password: false,
            has_dj_password: false,
        }
    }
}

const COLUMNS: &str = "id, name, host, port, sid, password, mount, codec, bitrate_kbps, \
    sample_rate_hz, mono, admin_user, admin_password, mode, dj_user, \
    dj_password, dj_port, dj_protocol, dj_mount, stream_name, genre, station_url, is_public";

fn from_row(row: &Row<'_>) -> Result<ProfileRecord> {
    Ok(ProfileRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get::<_, u32>(3)? as u16,
        sid: row.get::<_, u32>(4)?,
        password: row.get(5)?,
        mount: row.get(6)?,
        codec: row.get(7)?,
        bitrate_kbps: row.get(8)?,
        sample_rate_hz: row.get(9)?,
        mono: row.get::<_, i64>(10)? != 0,
        admin_user: row.get(11)?,
        admin_password: row.get(12)?,
        mode: row.get(13)?,
        dj_user: row.get(14)?,
        dj_password: row.get(15)?,
        dj_port: row.get::<_, Option<u32>>(16)?.map(|p| p as u16),
        dj_protocol: row.get(17)?,
        dj_mount: row.get(18)?,
        stream_name: row.get::<_, Option<String>>(19)?.unwrap_or_default(),
        genre: row.get::<_, Option<String>>(20)?.unwrap_or_default(),
        station_url: row.get::<_, Option<String>>(21)?.unwrap_or_default(),
        is_public: row.get::<_, Option<i64>>(22)?.map(|v| v != 0).unwrap_or(true),
        has_password: false,
        has_dj_password: false,
    })
}

pub struct ProfileDb {
    conn: Connection,
}

impl ProfileDb {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                sid INTEGER NOT NULL,
                password TEXT NOT NULL,
                mount TEXT NOT NULL,
                codec TEXT NOT NULL,
                bitrate_kbps INTEGER NOT NULL,
                sample_rate_hz INTEGER NOT NULL,
                mono INTEGER NOT NULL,
                admin_user TEXT,
                admin_password TEXT,
                mode TEXT NOT NULL DEFAULT 'SOURCE',
                dj_user TEXT,
                dj_password TEXT,
                dj_port INTEGER,
                dj_protocol TEXT NOT NULL DEFAULT 'ULTRAVOX',
                dj_mount TEXT
            );",
            [],
        )?;
        // Columns added after the first release.
        self.add_column_if_missing("dj_password", "TEXT")?;
        self.add_column_if_missing("stream_name", "TEXT")?;
        self.add_column_if_missing("genre", "TEXT")?;
        self.add_column_if_missing("station_url", "TEXT")?;
        self.add_column_if_missing("is_public", "INTEGER NOT NULL DEFAULT 1")?;
        Ok(())
    }

    fn add_column_if_missing(&self, name: &str, decl: &str) -> Result<()> {
        let has_col: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('profiles') WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;
        if has_col == 0 {
            self.conn
                .execute(&format!("ALTER TABLE profiles ADD COLUMN {name} {decl}"), [])?;
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<ProfileRecord>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM profiles ORDER BY id ASC"))?;
        let rows = stmt.query_map([], from_row)?;
        rows.collect()
    }

    pub fn get(&self, id: i64) -> Result<Option<ProfileRecord>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM profiles WHERE id = ?1"))?;
        let mut rows = stmt.query_map(params![id], from_row)?;
        rows.next().transpose()
    }

    pub fn insert(&self, p: &ProfileRecord) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO profiles (name, host, port, sid, password, mount, codec, bitrate_kbps, \
                                   sample_rate_hz, mono, admin_user, admin_password, mode, dj_user, \
                                   dj_password, dj_port, dj_protocol, dj_mount, stream_name, genre, \
                                   station_url, is_public) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, \
                     ?19, ?20, ?21, ?22)",
            params![
                p.name,
                p.host,
                p.port as u32,
                p.sid,
                p.password,
                p.mount,
                p.codec,
                p.bitrate_kbps,
                p.sample_rate_hz,
                p.mono as i64,
                p.admin_user,
                p.admin_password,
                p.mode,
                p.dj_user,
                p.dj_password,
                p.dj_port.map(|port| port as u32),
                p.dj_protocol,
                p.dj_mount,
                p.stream_name,
                p.genre,
                p.station_url,
                p.is_public as i64,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update(&self, p: &ProfileRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE profiles SET name = ?1, host = ?2, port = ?3, sid = ?4, password = ?5, \
                                 mount = ?6, codec = ?7, bitrate_kbps = ?8, sample_rate_hz = ?9, \
                                 mono = ?10, admin_user = ?11, admin_password = ?12, mode = ?13, \
                                 dj_user = ?14, dj_password = ?15, dj_port = ?16, dj_protocol = ?17, \
                                 dj_mount = ?18, stream_name = ?19, genre = ?20, station_url = ?21, \
                                 is_public = ?22 \
             WHERE id = ?23",
            params![
                p.name,
                p.host,
                p.port as u32,
                p.sid,
                p.password,
                p.mount,
                p.codec,
                p.bitrate_kbps,
                p.sample_rate_hz,
                p.mono as i64,
                p.admin_user,
                p.admin_password,
                p.mode,
                p.dj_user,
                p.dj_password,
                p.dj_port.map(|port| port as u32),
                p.dj_protocol,
                p.dj_mount,
                p.stream_name,
                p.genre,
                p.station_url,
                p.is_public as i64,
                p.id,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM profiles WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.conn.execute("DELETE FROM profiles", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_db_crud() {
        let db = ProfileDb::open_in_memory().unwrap();
        assert_eq!(db.list().unwrap().len(), 0);

        let p = ProfileRecord {
            name: "Live FM".into(),
            host: "radio.example.com".into(),
            password: "pass".into(),
            mount: "/live".into(),
            genre: "Talk".into(),
            ..Default::default()
        };

        let id = db.insert(&p).unwrap();
        assert!(id > 0);

        let mut loaded = db.get(id).unwrap().expect("found");
        assert_eq!(loaded.name, "Live FM");
        assert_eq!(loaded.genre, "Talk");
        assert!(loaded.is_public);

        loaded.name = "Live FM Updated".into();
        loaded.is_public = false;
        db.update(&loaded).unwrap();

        let updated = db.get(id).unwrap().expect("found updated");
        assert_eq!(updated.name, "Live FM Updated");
        assert!(!updated.is_public);

        db.delete(id).unwrap();
        assert!(db.get(id).unwrap().is_none());
    }

    #[test]
    fn legacy_table_is_migrated() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE profiles (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, \
             host TEXT NOT NULL, port INTEGER NOT NULL, sid INTEGER NOT NULL, password TEXT NOT NULL, \
             mount TEXT NOT NULL, codec TEXT NOT NULL, bitrate_kbps INTEGER NOT NULL, \
             sample_rate_hz INTEGER NOT NULL, mono INTEGER NOT NULL, admin_user TEXT, \
             admin_password TEXT, mode TEXT NOT NULL DEFAULT 'SOURCE', dj_user TEXT, dj_port INTEGER, \
             dj_protocol TEXT NOT NULL DEFAULT 'ULTRAVOX', dj_mount TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO profiles (name, host, port, sid, password, mount, codec, bitrate_kbps, \
             sample_rate_hz, mono) VALUES ('Old', 'h', 8000, 1, 'p', '', 'MP3', 64, 44100, 1)",
            [],
        )
        .unwrap();
        let db = ProfileDb { conn };
        db.init().unwrap();
        let p = &db.list().unwrap()[0];
        assert_eq!(p.name, "Old");
        assert_eq!(p.stream_name, "");
        assert!(p.is_public);
        assert_eq!(p.dj_password, None);
    }

    #[test]
    fn serde_defaults_match_db_defaults() {
        let p: ProfileRecord = serde_json::from_str(r#"{"name":"x","host":"h","port":8000}"#).unwrap();
        assert_eq!(p.mode, "SOURCE");
        assert_eq!(p.dj_protocol, "ULTRAVOX");
    }
}
