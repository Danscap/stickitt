//! SQLite persistence: notes, the lists they belong to, profiles (named sets of active lists)
//! and small settings. Note writes are batched into a single transaction.

use crate::reminder::{Reminder, Repeat};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct NoteRecord {
    pub id: i64,
    pub body: String,
    /// Outer position in physical pixels (None → let the OS place it).
    pub pos: Option<(i32, i32)>,
    /// Inner size in logical pixels.
    pub size: (f64, f64),
    /// A preset index or `theme::CUSTOM + hue`.
    pub color: u16,
    pub mode: u8,
    pub zoom: u16,
    pub reminder: Option<Reminder>,
    /// Page icon (an emoji), empty for none.
    pub icon: String,
    pub list: i64,
    /// Virtual desktop the note lives on (a GUID string), when known.
    pub desktop: Option<String>,
    /// Closed with ×: kept (and listed in the dock) but not shown until reopened.
    pub hidden: bool,
    /// How the note is shown: 0 = full page, 1 = compact (one line).
    pub view: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    /// Lists shown (and notes visible) while this profile is active.
    pub lists: Vec<i64>,
}

pub struct Store {
    conn: Connection,
}

const SCHEMA_VERSION: i32 = 6;
/// The list every note falls back to; it can be renamed but not deleted.
pub const DEFAULT_LIST: i64 = 1;
/// The profile showing every list; created on first run.
pub const DEFAULT_PROFILE: i64 = 1;

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if (1..6).contains(&version) {
            // VACUUM INTO includes committed WAL contents, unlike copying the .db alone.
            let backup = path.with_extension(format!(
                "pre-dates-{}.db",
                chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
        }
        Self::init(conn)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE notes (
                    id INTEGER PRIMARY KEY,
                    body TEXT NOT NULL DEFAULT '',
                    x INTEGER, y INTEGER,
                    w REAL NOT NULL, h REAL NOT NULL,
                    color INTEGER NOT NULL DEFAULT 0,
                    mode INTEGER NOT NULL DEFAULT 0,
                    zoom INTEGER NOT NULL DEFAULT 100,
                    reminder_due INTEGER,
                    reminder_anchor INTEGER,
                    reminder_repeat TEXT,
                    deleted_at INTEGER,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );",
            )?;
        }
        if version < 2 {
            conn.execute_batch("ALTER TABLE notes ADD COLUMN icon TEXT NOT NULL DEFAULT '';")?;
        }
        if version < 3 {
            conn.execute_batch(
                "CREATE TABLE lists (id INTEGER PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE profiles (id INTEGER PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE profile_lists (
                    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                    list_id INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
                    PRIMARY KEY (profile_id, list_id)
                 );
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO lists (id, name) VALUES (1, 'Notes');
                 INSERT INTO profiles (id, name) VALUES (1, 'All');
                 INSERT INTO profile_lists VALUES (1, 1);
                 ALTER TABLE notes ADD COLUMN list_id INTEGER NOT NULL DEFAULT 1;
                 ALTER TABLE notes ADD COLUMN desktop TEXT;",
            )?;
        }
        if version < 4 {
            conn.execute_batch("ALTER TABLE notes ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;")?;
        }
        if version < 5 {
            conn.execute_batch("ALTER TABLE notes ADD COLUMN view INTEGER NOT NULL DEFAULT 0;")?;
        }
        if version < 6 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(
                "CREATE TABLE note_dates (
                note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
                local_day TEXT NOT NULL
            );",
            )?;
            let today = chrono::Local::now().date_naive().to_string();
            tx.execute("INSERT INTO note_dates SELECT id, ?1 FROM notes", [&today])?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    // ---- notes ------------------------------------------------------------------------------

    /// Notes that are not in the trash.
    pub fn load(&self) -> rusqlite::Result<Vec<NoteRecord>> {
        self.notes_where("deleted_at IS NULL ORDER BY id", [])
    }

    /// The trash, most recently deleted first.
    pub fn trashed(&self, limit: i64) -> rusqlite::Result<Vec<NoteRecord>> {
        self.notes_where("deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC LIMIT ?1", [limit])
    }

    fn notes_where<P: rusqlite::Params>(&self, filter: &str, args: P) -> rusqlite::Result<Vec<NoteRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, body, x, y, w, h, color, mode, zoom,
                    reminder_due, reminder_anchor, reminder_repeat, icon, list_id, desktop, hidden, view
             FROM notes WHERE {filter}"
        ))?;
        let rows = stmt.query_map(args, |r| {
            let x: Option<i32> = r.get(2)?;
            let y: Option<i32> = r.get(3)?;
            let due: Option<i64> = r.get(9)?;
            let anchor: Option<i64> = r.get(10)?;
            let repeat: Option<String> = r.get(11)?;
            Ok(NoteRecord {
                id: r.get(0)?,
                body: r.get(1)?,
                pos: x.zip(y),
                size: (r.get(4)?, r.get(5)?),
                color: r.get(6)?,
                mode: r.get(7)?,
                zoom: r.get(8)?,
                reminder: due.map(|due| Reminder {
                    due,
                    anchor: anchor.unwrap_or(due),
                    repeat: Repeat::decode(repeat.as_deref().unwrap_or("")),
                }),
                icon: r.get(12)?,
                list: r.get(13)?,
                desktop: r.get(14)?,
                hidden: r.get(15)?,
                view: r.get(16)?,
            })
        })?;
        rows.collect()
    }

    pub fn insert(&self, rec: &NoteRecord) -> rusqlite::Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO notes (body, w, h, color, mode, zoom, icon, list_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![rec.body, rec.size.0, rec.size.1, rec.color, rec.mode, rec.zoom, rec.icon, rec.list],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO note_dates VALUES (?1, ?2)",
            params![id, chrono::Local::now().date_naive().to_string()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn note_dates(&self) -> rusqlite::Result<std::collections::HashMap<i64, chrono::NaiveDate>> {
        let mut stmt = self.conn.prepare("SELECT note_id, local_day FROM note_dates")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut dates = std::collections::HashMap::new();
        for row in rows {
            let (id, day) = row?;
            if let Ok(day) = day.parse() {
                dates.insert(id, day);
            }
        }
        Ok(dates)
    }

    /// Atomic soft deletion. No records are purged; callers offer the existing grouped Undo.
    pub fn trash_many(&mut self, ids: &[i64]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        for id in ids {
            tx.execute("UPDATE notes SET deleted_at=unixepoch() WHERE id=?1 AND deleted_at IS NULL", [id])?;
        }
        tx.commit()
    }

    pub fn save_all<'a>(&mut self, recs: impl IntoIterator<Item = &'a NoteRecord>) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE notes SET body=?2, x=?3, y=?4, w=?5, h=?6, color=?7, mode=?8, zoom=?9,
                    reminder_due=?10, reminder_anchor=?11, reminder_repeat=?12, icon=?13, list_id=?14,
                    desktop=COALESCE(?15, desktop), hidden=?16, view=?17, updated_at=unixepoch()
                 WHERE id=?1",
            )?;
            for r in recs {
                let rem = r.reminder.as_ref();
                stmt.execute(params![
                    r.id,
                    r.body,
                    r.pos.map(|p| p.0),
                    r.pos.map(|p| p.1),
                    r.size.0,
                    r.size.1,
                    r.color,
                    r.mode,
                    r.zoom,
                    rem.map(|r| r.due),
                    rem.map(|r| r.anchor),
                    rem.map(|r| r.repeat.encode()),
                    r.icon,
                    r.list,
                    r.desktop,
                    r.hidden,
                    r.view,
                ])?;
            }
        }
        tx.commit()
    }

    /// Soft delete: the note goes to the trash (restorable from the manager later).
    pub fn trash(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("UPDATE notes SET deleted_at=unixepoch() WHERE id=?1", [id])?;
        Ok(())
    }

    /// Takes a note out of the trash and returns it.
    pub fn restore(&self, id: i64) -> rusqlite::Result<Option<NoteRecord>> {
        self.forget_closed(id);
        self.conn.execute("UPDATE notes SET deleted_at=NULL WHERE id=?1", [id])?;
        Ok(self.notes_where("id=?1", [id])?.pop())
    }

    /// Deletes one trashed note for good.
    pub fn purge(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM notes WHERE id=?1 AND deleted_at IS NOT NULL", [id])?;
        Ok(())
    }

    /// Deletes trashed notes for good: all of them, or those trashed more than `age` seconds ago.
    pub fn empty_trash(&self, age: Option<i64>) -> rusqlite::Result<usize> {
        match age {
            None => self.conn.execute("DELETE FROM notes WHERE deleted_at IS NOT NULL", []),
            Some(age) => self
                .conn
                .execute("DELETE FROM notes WHERE deleted_at IS NOT NULL AND deleted_at < unixepoch() - ?1", [age]),
        }
    }

    // ---- lists & profiles -------------------------------------------------------------------

    pub fn lists(&self) -> rusqlite::Result<Vec<List>> {
        let mut stmt = self.conn.prepare("SELECT id, name FROM lists ORDER BY position, id")?;
        let rows = stmt.query_map([], |r| Ok(List { id: r.get(0)?, name: r.get(1)? }))?;
        rows.collect()
    }

    pub fn profiles(&self) -> rusqlite::Result<Vec<Profile>> {
        let mut stmt = self.conn.prepare("SELECT id, name FROM profiles ORDER BY position, id")?;
        let mut profiles: Vec<Profile> = stmt
            .query_map([], |r| Ok(Profile { id: r.get(0)?, name: r.get(1)?, lists: Vec::new() }))?
            .collect::<rusqlite::Result<_>>()?;
        let mut links = self.conn.prepare(
            "SELECT pl.list_id FROM profile_lists pl JOIN lists l ON l.id = pl.list_id
             WHERE pl.profile_id = ?1 ORDER BY l.position, l.id",
        )?;
        for p in &mut profiles {
            p.lists = links.query_map([p.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        }
        Ok(profiles)
    }

    /// Creates a list and adds it to `profile` so it shows up right away.
    pub fn create_list(&self, name: &str, profile: i64) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO lists (name, position) VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM lists))",
            [name],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute("INSERT OR IGNORE INTO profile_lists VALUES (?1, ?2)", [profile, id])?;
        // The catch-all profile always shows every list.
        self.conn.execute("INSERT OR IGNORE INTO profile_lists VALUES (?1, ?2)", [DEFAULT_PROFILE, id])?;
        Ok(id)
    }

    /// Deletes a list. Its notes move to the default list, and every profile that showed the
    /// list now shows the default list too, so they stay in sight. With `trash_notes` they go to
    /// the trash instead (and come back into the default list if restored). Either way no
    /// profile is left showing no list at all.
    pub fn delete_list(&self, id: i64, trash_notes: bool) -> rusqlite::Result<()> {
        if id == DEFAULT_LIST {
            return Ok(());
        }
        if trash_notes {
            self.conn
                .execute("UPDATE notes SET deleted_at = unixepoch() WHERE list_id = ?1 AND deleted_at IS NULL", [id])?;
        }
        self.conn.execute("UPDATE notes SET list_id = ?1 WHERE list_id = ?2", [DEFAULT_LIST, id])?;
        let shown_by = if trash_notes {
            // Only the profiles that would be left empty.
            "SELECT profile_id, ?1 FROM profile_lists pl WHERE list_id = ?2 AND NOT EXISTS
             (SELECT 1 FROM profile_lists o WHERE o.profile_id = pl.profile_id AND o.list_id != ?2)"
        } else {
            "SELECT profile_id, ?1 FROM profile_lists WHERE list_id = ?2"
        };
        self.conn.execute(&format!("INSERT OR IGNORE INTO profile_lists {shown_by}"), [DEFAULT_LIST, id])?;
        self.conn.execute("DELETE FROM profile_lists WHERE list_id = ?1", [id])?;
        self.conn.execute("DELETE FROM lists WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Creates a profile showing `lists`.
    pub fn create_profile(&self, name: &str, lists: &[i64]) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO profiles (name, position) VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM profiles))",
            [name],
        )?;
        let id = self.conn.last_insert_rowid();
        for l in lists {
            self.conn.execute("INSERT OR IGNORE INTO profile_lists VALUES (?1, ?2)", [id, *l])?;
        }
        Ok(id)
    }

    pub fn delete_profile(&self, id: i64) -> rusqlite::Result<()> {
        if id != DEFAULT_PROFILE {
            self.conn.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    /// Shows or hides `list` in `profile`. A profile always shows at least one list: hiding the
    /// last one shows the default list instead (and the default list can't be the one hidden).
    pub fn set_profile_list(&self, profile: i64, list: i64, on: bool) -> rusqlite::Result<()> {
        if on {
            self.conn.execute("INSERT OR IGNORE INTO profile_lists VALUES (?1, ?2)", [profile, list])?;
            return Ok(());
        }
        let others: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM profile_lists WHERE profile_id = ?1 AND list_id != ?2",
            [profile, list],
            |r| r.get(0),
        )?;
        if others == 0 {
            if list == DEFAULT_LIST {
                return Ok(());
            }
            self.conn.execute("INSERT OR IGNORE INTO profile_lists VALUES (?1, ?2)", [profile, DEFAULT_LIST])?;
        }
        self.conn.execute("DELETE FROM profile_lists WHERE profile_id = ?1 AND list_id = ?2", [profile, list])?;
        Ok(())
    }

    // ---- settings ---------------------------------------------------------------------------

    /// Only explicit single-note closes enter this MRU. Stored in the existing settings
    /// table, so old databases and binaries need no additional schema migration.
    pub fn remember_closed(&self, id: i64) {
        let mut ids = self.closed_notes();
        ids.retain(|old| *old != id);
        ids.insert(0, id);
        self.write_closed_notes(&ids);
    }

    pub fn forget_closed(&self, id: i64) {
        let mut ids = self.closed_notes();
        let len = ids.len();
        ids.retain(|old| *old != id);
        if ids.len() != len {
            self.write_closed_notes(&ids);
        }
    }

    fn closed_notes(&self) -> Vec<i64> {
        self.setting("closed_notes")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .collect()
    }

    fn write_closed_notes(&self, ids: &[i64]) {
        self.set_setting("closed_notes", &ids.iter().map(i64::to_string).collect::<Vec<_>>().join(","));
    }

    /// `available` comes from live windows in the current profile, excluding closing
    /// windows. Database checks also reject Trash, purged and already reopened notes.
    pub fn last_closed(&self, available: &[i64]) -> Option<i64> {
        self.closed_notes().into_iter().find(|id| {
            available.contains(id)
                && self
                    .conn
                    .query_row("SELECT hidden != 0 AND deleted_at IS NULL FROM notes WHERE id=?1", [id], |r| {
                        r.get::<_, bool>(0)
                    })
                    .unwrap_or(false)
        })
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let _ = self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        );
    }

    #[cfg(test)]
    pub fn count(&self) -> rusqlite::Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL", [], |r| r.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close_fixture(store: &mut Store, id: i64) {
        let mut rec = store.load().unwrap().into_iter().find(|r| r.id == id).unwrap();
        rec.hidden = true;
        store.save_all([&rec]).unwrap();
        store.remember_closed(id);
    }

    #[test]
    fn closed_order_is_explicit_latest_first_and_deduplicated() {
        let mut store = Store::in_memory().unwrap();
        let a = store.insert(&note()).unwrap();
        let b = store.insert(&note()).unwrap();
        assert_eq!(store.last_closed(&[a, b]), None);
        // Merely being hidden (e.g. old data) does not invent a deliberate close.
        store.conn.execute("UPDATE notes SET hidden=1", []).unwrap();
        assert_eq!(store.last_closed(&[a, b]), None);
        close_fixture(&mut store, a);
        close_fixture(&mut store, b);
        assert_eq!(store.last_closed(&[a, b]), Some(b));
        close_fixture(&mut store, a);
        assert_eq!(store.closed_notes(), vec![a, b]);
        assert_eq!(store.last_closed(&[a, b]), Some(a));
        store.forget_closed(a);
        assert_eq!(store.last_closed(&[a, b]), Some(b));
    }

    #[test]
    fn closed_selection_excludes_reopened_deleted_restored_and_unavailable_notes() {
        let mut store = Store::in_memory().unwrap();
        let a = store.insert(&note()).unwrap();
        let b = store.insert(&note()).unwrap();
        close_fixture(&mut store, a);
        close_fixture(&mut store, b);
        assert_eq!(store.last_closed(&[]), None);
        assert_eq!(store.last_closed(&[a]), Some(a)); // b has no available window
        let mut rec = store.load().unwrap().into_iter().find(|n| n.id == b).unwrap();
        rec.hidden = false;
        store.save_all([&rec]).unwrap();
        assert_eq!(store.last_closed(&[a, b]), Some(a));
        close_fixture(&mut store, b);
        store.trash_many(&[b]).unwrap();
        assert_eq!(store.last_closed(&[a, b]), Some(a));
        store.restore(b).unwrap();
        assert_eq!(store.last_closed(&[a, b]), Some(a), "Trash undo must not restore close history");
        store.trash(a).unwrap();
        store.purge(a).unwrap();
        assert_eq!(store.last_closed(&[a, b]), None);
        store.set_setting("closed_notes", "invalid,-1,999999");
        assert_eq!(store.last_closed(&[a, b, 999999]), None);
    }

    #[test]
    fn closed_selection_preserves_other_profile_history() {
        let mut store = Store::in_memory().unwrap();
        let a = store.insert(&note()).unwrap();
        let profile = store.create_profile("Other fixture profile", &[]).unwrap();
        let other_list = store.create_list("Other fixture list", profile).unwrap();
        let mut rec = note();
        rec.list = other_list;
        let b = store.insert(&rec).unwrap();
        close_fixture(&mut store, a);
        close_fixture(&mut store, b);
        let available =
            |list| store.load().unwrap().into_iter().filter(|r| r.list == list).map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(store.last_closed(&available(DEFAULT_LIST)), Some(a));
        assert_eq!(store.last_closed(&available(other_list)), Some(b));
        store.forget_closed(a);
        assert_eq!(store.last_closed(&available(DEFAULT_LIST)), None);
        assert_eq!(store.last_closed(&available(other_list)), Some(b));
    }

    #[test]
    fn closed_order_survives_restart_without_schema_change() {
        let path = std::env::temp_dir().join(format!(
            "stickit-closed-{}-{}.db",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap()
        ));
        let (a, b);
        {
            let mut store = Store::open(&path).unwrap();
            a = store.insert(&note()).unwrap();
            b = store.insert(&note()).unwrap();
            close_fixture(&mut store, a);
            close_fixture(&mut store, b);
        }
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.last_closed(&[a, b]), Some(b));
            assert_eq!(store.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)).unwrap(), 6);
            store.forget_closed(b);
        }
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.last_closed(&[a, b]), Some(a));
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_bulk_trash_rolls_back_every_note() {
        let mut store = Store::in_memory().unwrap();
        let a = store.insert(&note()).unwrap();
        let b = store.insert(&note()).unwrap();
        store
            .conn
            .execute_batch(&format!(
                "CREATE TRIGGER refuse_second BEFORE UPDATE OF deleted_at ON notes
            WHEN NEW.id={b} BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;"
            ))
            .unwrap();
        assert!(store.trash_many(&[a, b]).is_err());
        assert_eq!(store.load().unwrap().len(), 2);
        assert!(store.trashed(10).unwrap().is_empty());
        store.conn.execute_batch("DROP TRIGGER refuse_second").unwrap();
        store.trash_many(&[a, b]).unwrap();
        assert!(store.load().unwrap().is_empty());
        for id in [a, b] {
            assert_eq!(store.restore(id).unwrap().unwrap().body, "hi");
        }
        assert_eq!(store.load().unwrap().len(), 2);
    }

    #[test]
    fn effective_dates_migrate_once_and_preserve_raw_metadata() {
        let store = Store::in_memory().unwrap();
        let id = store.insert(&note()).unwrap();
        store.conn.execute("UPDATE notes SET created_at=123, updated_at=456 WHERE id=?1", [id]).unwrap();
        store.conn.execute_batch("DROP TABLE note_dates; PRAGMA user_version=5;").unwrap();
        let store = Store::init(store.conn).unwrap();
        assert_eq!(store.note_dates().unwrap()[&id], chrono::Local::now().date_naive());
        store.conn.execute("UPDATE note_dates SET local_day='2020-01-02'", []).unwrap();
        let mut store = Store::init(store.conn).unwrap();
        assert_eq!(store.note_dates().unwrap()[&id].to_string(), "2020-01-02");
        let raw: (i64, i64) = store
            .conn
            .query_row("SELECT created_at, updated_at FROM notes", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(raw, (123, 456));
        store.trash_many(&[id]).unwrap();
        store.restore(id).unwrap();
        assert_eq!(store.note_dates().unwrap()[&id].to_string(), "2020-01-02");
    }

    #[test]
    fn upgrade_makes_reopenable_backup() {
        let dir = std::env::temp_dir().join(format!(
            "stickit-upgrade-{}-{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fixture.db");
        {
            let store = Store::open(&path).unwrap();
            store.insert(&note()).unwrap();
            store.conn.execute_batch("DROP TABLE note_dates; PRAGMA user_version=5;").unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.load().unwrap().len(), 1);
        drop(store);
        let backup = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().contains("pre-dates-"))
            .unwrap()
            .path();
        let conn = Connection::open(&backup).unwrap();
        assert_eq!(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)).unwrap(), 5);
        assert_eq!(conn.query_row("SELECT body FROM notes", [], |r| r.get::<_, String>(0)).unwrap(), "hi");
        drop(conn);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn trash_and_restore() {
        let store = Store::in_memory().unwrap();
        let a = store.insert(&note()).unwrap();
        let b = store.insert(&note()).unwrap();
        store.trash(a).unwrap();
        store.trash(b).unwrap();
        assert_eq!(store.load().unwrap().len(), 0);
        assert_eq!(store.trashed(10).unwrap().len(), 2);
        let back = store.restore(a).unwrap().unwrap();
        assert_eq!((back.id, back.icon.as_str()), (a, "📌"));
        assert_eq!(store.load().unwrap().len(), 1);
        // Just trashed, so not old enough to purge yet.
        assert_eq!(store.empty_trash(Some(3600)).unwrap(), 0);
        // Purging only touches the trash.
        store.purge(a).unwrap();
        assert_eq!(store.load().unwrap().len(), 1);
        assert_eq!(store.empty_trash(None).unwrap(), 1);
        assert!(store.trashed(10).unwrap().is_empty());
        assert_eq!(store.load().unwrap().len(), 1);
    }

    fn note() -> NoteRecord {
        NoteRecord {
            id: 0,
            body: "hi".into(),
            pos: None,
            size: (260.0, 240.0),
            color: 2,
            mode: 1,
            zoom: 100,
            reminder: None,
            icon: "📌".into(),
            list: DEFAULT_LIST,
            desktop: None,
            hidden: false,
            view: 0,
        }
    }

    #[test]
    fn roundtrip() {
        let mut store = Store::in_memory().unwrap();
        let mut rec = note();
        rec.id = store.insert(&rec).unwrap();
        rec.pos = Some((10, -20));
        rec.reminder = Some(Reminder { due: 5, anchor: 4, repeat: Repeat::Every(60) });
        rec.desktop = Some("{A}".into());
        rec.hidden = true;
        rec.view = 1;
        store.save_all([&rec]).unwrap();
        // A save without a known desktop keeps the stored one.
        rec.desktop = None;
        store.save_all([&rec]).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].pos, Some((10, -20)));
        assert_eq!(loaded[0].reminder, rec.reminder);
        assert_eq!(loaded[0].icon, "📌");
        assert_eq!(loaded[0].desktop.as_deref(), Some("{A}"));
        assert!(loaded[0].hidden);
        assert_eq!(loaded[0].view, 1);
        store.trash(rec.id).unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn lists_and_profiles() {
        let mut store = Store::in_memory().unwrap();
        let work = store.create_list("Work", DEFAULT_PROFILE).unwrap();
        let focus = store.create_profile("Focus", &[work]).unwrap();
        let profiles = store.profiles().unwrap();
        assert_eq!(profiles[0].lists, vec![DEFAULT_LIST, work]);
        assert_eq!(profiles[1], Profile { id: focus, name: "Focus".into(), lists: vec![work] });

        let mut rec = note();
        rec.list = work;
        rec.id = store.insert(&rec).unwrap();
        store.save_all([&rec]).unwrap();
        store.delete_list(work, false).unwrap();
        assert_eq!(store.load().unwrap()[0].list, DEFAULT_LIST);
        // The profile that showed the list now shows where its notes went.
        assert_eq!(store.profiles().unwrap()[1].lists, vec![DEFAULT_LIST]);

        store.set_setting("profile", "2");
        store.set_setting("profile", "3");
        assert_eq!(store.setting("profile").as_deref(), Some("3"));
    }

    #[test]
    fn deleting_a_list_with_its_notes() {
        let store = Store::in_memory().unwrap();
        let work = store.create_list("Work", DEFAULT_PROFILE).unwrap();
        let home = store.create_list("Home", DEFAULT_PROFILE).unwrap();
        let focus = store.create_profile("Focus", &[work]).unwrap();
        let both = store.create_profile("Both", &[work, home]).unwrap();
        let id = store.insert(&NoteRecord { list: work, ..note() }).unwrap();
        let lists_of = |p: i64| store.profiles().unwrap().into_iter().find(|x| x.id == p).map(|x| x.lists);
        store.delete_list(work, true).unwrap();
        // The note is in the trash, and comes back into the default list.
        assert!(store.load().unwrap().iter().all(|n| n.id != id));
        assert_eq!(store.restore(id).unwrap().map(|n| n.list), Some(DEFAULT_LIST));
        // A profile left with nothing shows the default list; one with other lists keeps them.
        assert_eq!(lists_of(focus), Some(vec![DEFAULT_LIST]));
        assert_eq!(lists_of(both), Some(vec![home]));
    }

    #[test]
    fn a_profile_always_shows_a_list() {
        let store = Store::in_memory().unwrap();
        let work = store.create_list("Work", DEFAULT_PROFILE).unwrap();
        let focus = store.create_profile("Focus", &[work]).unwrap();
        let lists_of = |p: i64| store.profiles().unwrap().into_iter().find(|x| x.id == p).map(|x| x.lists);
        // Hiding the last list shows the default one instead.
        store.set_profile_list(focus, work, false).unwrap();
        assert_eq!(lists_of(focus), Some(vec![DEFAULT_LIST]));
        // ...which can't be hidden while it's the only one.
        store.set_profile_list(focus, DEFAULT_LIST, false).unwrap();
        assert_eq!(lists_of(focus), Some(vec![DEFAULT_LIST]));
        store.set_profile_list(focus, work, true).unwrap();
        store.set_profile_list(focus, DEFAULT_LIST, false).unwrap();
        assert_eq!(lists_of(focus), Some(vec![work]));
    }

    #[test]
    fn toggling_lists_and_protected_defaults() {
        let store = Store::in_memory().unwrap();
        let work = store.create_list("Work", DEFAULT_PROFILE).unwrap();
        let focus = store.create_profile("Focus", &[work]).unwrap();
        let id = store.insert(&NoteRecord { list: work, ..note() }).unwrap();
        let lists_of = |p: i64| store.profiles().unwrap().into_iter().find(|x| x.id == p).map(|x| x.lists);
        assert_eq!(lists_of(focus), Some(vec![work]));
        // The catch-all profile shows every list.
        assert!(lists_of(DEFAULT_PROFILE).unwrap().contains(&work));
        store.set_profile_list(focus, DEFAULT_LIST, true).unwrap();
        assert_eq!(lists_of(focus).unwrap().len(), 2);
        store.set_profile_list(focus, DEFAULT_LIST, false).unwrap();
        assert_eq!(lists_of(focus), Some(vec![work]));
        // Deleting a list moves its notes to the default list.
        store.delete_list(work, false).unwrap();
        assert_eq!(store.load().unwrap().iter().find(|n| n.id == id).map(|n| n.list), Some(DEFAULT_LIST));
        // The defaults can't be deleted.
        store.delete_list(DEFAULT_LIST, true).unwrap();
        store.delete_profile(DEFAULT_PROFILE).unwrap();
        assert!(store.lists().unwrap().iter().any(|l| l.id == DEFAULT_LIST));
        assert!(lists_of(DEFAULT_PROFILE).is_some());
        store.delete_profile(focus).unwrap();
        assert_eq!(lists_of(focus), None);
    }

    #[test]
    fn file_store_persists_notes_and_settings() {
        let path = std::env::temp_dir().join(format!("stickit-test-{}.db", std::process::id()));
        let cleanup = || {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
            }
        };
        cleanup();
        {
            let store = Store::open(&path).unwrap();
            store.insert(&note()).unwrap();
            store.set_setting("dock_side", "left");
            store.set_setting("dock_side", "right");
        }
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.load().unwrap().len(), 1);
            assert_eq!(store.setting("dock_side").as_deref(), Some("right"));
            assert_eq!(store.setting("missing"), None);
        }
        cleanup();
    }
}
