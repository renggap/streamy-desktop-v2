//! SQLite persistence for server profiles.
//! Schema matches Android `ProfileEntity`.

use std::path::Path;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};

fn default_sid() -> u32 { 1 }
fn default_codec() -> String { "MP3".to_string() }
fn default_bitrate() -> u32 { 128 }
fn default_sample_rate() -> u32 { 44100 }
fn default_mode() -> String { "DJ".to_string() }
fn default_dj_protocol() -> String { "SHOUTCAST".to_string() }

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
    pub dj_port: Option<u16>,
    #[serde(alias = "protocol", default = "default_dj_protocol")]
    pub dj_protocol: String,
    #[serde(default)]
    pub dj_mount: Option<String>,
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
                dj_port INTEGER,
                dj_protocol TEXT NOT NULL DEFAULT 'ULTRAVOX',
                dj_mount TEXT
            );",
            [],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<ProfileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, host, port, sid, password, mount, codec, bitrate_kbps, \
                    sample_rate_hz, mono, admin_user, admin_password, mode, dj_user, \
                    dj_port, dj_protocol, dj_mount \
             FROM profiles ORDER BY id ASC"
        )?;

        let iter = stmt.query_map([], |row| {
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
                dj_port: row.get::<_, Option<u32>>(15)?.map(|p| p as u16),
                dj_protocol: row.get(16)?,
                dj_mount: row.get(17)?,
            })
        })?;

        let mut out = Vec::new();
        for r in iter {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get(&self, id: i64) -> Result<Option<ProfileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, host, port, sid, password, mount, codec, bitrate_kbps, \
                    sample_rate_hz, mono, admin_user, admin_password, mode, dj_user, \
                    dj_port, dj_protocol, dj_mount \
             FROM profiles WHERE id = ?1"
        )?;

        let mut iter = stmt.query_map(params![id], |row| {
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
                dj_port: row.get::<_, Option<u32>>(15)?.map(|p| p as u16),
                dj_protocol: row.get(16)?,
                dj_mount: row.get(17)?,
            })
        })?;

        if let Some(res) = iter.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn insert(&self, p: &ProfileRecord) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO profiles (name, host, port, sid, password, mount, codec, bitrate_kbps, \
                                   sample_rate_hz, mono, admin_user, admin_password, mode, dj_user, \
                                   dj_port, dj_protocol, dj_mount) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                if p.mono { 1 } else { 0 },
                p.admin_user,
                p.admin_password,
                p.mode,
                p.dj_user,
                p.dj_port.map(|port| port as u32),
                p.dj_protocol,
                p.dj_mount,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update(&self, p: &ProfileRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE profiles SET name = ?1, host = ?2, port = ?3, sid = ?4, password = ?5, \
                                 mount = ?6, codec = ?7, bitrate_kbps = ?8, sample_rate_hz = ?9, \
                                 mono = ?10, admin_user = ?11, admin_password = ?12, mode = ?13, \
                                 dj_user = ?14, dj_port = ?15, dj_protocol = ?16, dj_mount = ?17 \
             WHERE id = ?18",
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
                if p.mono { 1 } else { 0 },
                p.admin_user,
                p.admin_password,
                p.mode,
                p.dj_user,
                p.dj_port.map(|port| port as u32),
                p.dj_protocol,
                p.dj_mount,
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
            id: 0,
            name: "Live FM".into(),
            host: "radio.example.com".into(),
            port: 8000,
            sid: 1,
            password: "pass".into(),
            mount: "/live".into(),
            codec: "MP3".into(),
            bitrate_kbps: 128,
            sample_rate_hz: 44100,
            mono: true,
            admin_user: None,
            admin_password: None,
            mode: "SOURCE".into(),
            dj_user: None,
            dj_port: None,
            dj_protocol: "ULTRAVOX".into(),
            dj_mount: None,
        };

        let id = db.insert(&p).unwrap();
        assert!(id > 0);

        let mut loaded = db.get(id).unwrap().expect("found");
        assert_eq!(loaded.name, "Live FM");

        loaded.name = "Live FM Updated".into();
        db.update(&loaded).unwrap();

        let updated = db.get(id).unwrap().expect("found updated");
        assert_eq!(updated.name, "Live FM Updated");

        db.delete(id).unwrap();
        assert!(db.get(id).unwrap().is_none());
    }
}
