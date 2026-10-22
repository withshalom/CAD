//! Drawing locks and presence: the sidecar file beside an open drawing.
//! BlackCAD's is `.bdl` (beside `.blc` drawings); CADCraft's `.cdl` (beside
//! its `.cad` drawings) is the same file under its own name ([`LockKind`]).
//!
//! This plays the role AutoCAD's `.dwl` file plays for DWG: while someone
//! has a drawing open for editing, a small text file next to it says who,
//! on which machine, since when. Other copies of the app read it to open
//! the drawing read-only and show who holds it.
//!
//! Beyond `.dwl`, the lock carries a heartbeat. The holder refreshes it
//! every [`HEARTBEAT_SECS`]; a lock whose heartbeat is older than
//! [`STALE_AFTER_SECS`] is treated as left behind by a crash and can be
//! taken over, without anyone deleting files by hand. The same heartbeat
//! is what a live-collaboration layer will use for presence.
//!
//! Like `.dwl`, the lock is advisory: it coordinates copies of this app on
//! a shared folder; it does not stop other programs writing the drawing.
//!
//! Known limits, inherent to a lock made of a plain file: refresh and
//! release check the file and then act, so a takeover that lands in that
//! instant can be overwritten (the next refresh of either holder detects
//! it). Staleness compares wall clocks, so machines whose clocks differ by
//! more than [`STALE_AFTER_SECS`] can take over each other's live locks.
//! Cloud-sync folders may delay or reorder the file operations.
//!
//! Format (UTF-8 text, one `key=value` per line, unknown keys ignored):
//!
//! ```text
//! bdl 1
//! user=shalom
//! host=shop-pc
//! pid=4121
//! app=cad-core 0.2.0
//! session=5f0c9a1e2b7d4c83
//! opened=1790000000
//! heartbeat=1790000030
//! ```
//!
//! The first line names the kind of lock and its revision (`bdl 1`, or
//! `cdl 1` for CADCraft's). Readers take either: the two differ only in name.

use crate::error::{Error, ErrorKind, Result};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Whose lock: the lock file's extension, and the drawings that take it in
/// place of their own extension (`part.blc` → `part.bdl`; any other file
/// gets it appended, `part.dwg` → `part.dwg.bdl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockKind {
    pub lock: &'static str,
    pub native: &'static str,
}

impl LockKind {
    /// BlackCAD's: `.bdl` beside `.blc` drawings.
    pub const BLACKCAD: LockKind = LockKind { lock: "bdl", native: "blc" };
    /// CADCraft's: `.cdl` beside `.cad` drawings.
    pub const CADCRAFT: LockKind = LockKind { lock: "cdl", native: "cad" };
}

/// BlackCAD's lock extension (what the functions without a kind use).
pub const LOCK_EXTENSION: &str = LockKind::BLACKCAD.lock;
/// How often a holder should call [`DrawingLock::refresh`].
pub const HEARTBEAT_SECS: u64 = 30;
/// A lock not refreshed for this long is considered abandoned.
pub const STALE_AFTER_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LockInfo {
    pub user: String,
    pub host: String,
    pub pid: u32,
    pub app: String,
    pub session: String,
    /// Unix seconds.
    pub opened: u64,
    /// Unix seconds of the last refresh.
    pub heartbeat: u64,
}

impl LockInfo {
    pub fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.heartbeat) > STALE_AFTER_SECS
    }

    fn encode(&self, kind: LockKind) -> String {
        let clean = |s: &str| s.replace(['\r', '\n'], " ");
        format!(
            "{} 1\nuser={}\nhost={}\npid={}\napp={}\nsession={}\nopened={}\nheartbeat={}\n",
            kind.lock,
            clean(&self.user),
            clean(&self.host),
            self.pid,
            clean(&self.app),
            clean(&self.session),
            self.opened,
            self.heartbeat
        )
    }

    fn decode(text: &str) -> Option<LockInfo> {
        let mut lines = text.lines();
        if !matches!(lines.next()?.trim(), "bdl 1" | "cdl 1") {
            return None;
        }
        let mut i = LockInfo::default();
        let mut has_heartbeat = false;
        for line in lines {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "user" => i.user = v.to_string(),
                "host" => i.host = v.to_string(),
                "pid" => i.pid = v.trim().parse().unwrap_or(0),
                "app" => i.app = v.to_string(),
                "session" => i.session = v.to_string(),
                "opened" => i.opened = v.trim().parse().unwrap_or(0),
                "heartbeat" => {
                    if let Ok(t) = v.trim().parse() {
                        i.heartbeat = t;
                        has_heartbeat = true;
                    }
                }
                _ => {}
            }
        }
        // A lock cut short while being written is not trusted; the caller
        // dates it by the file's modification time instead.
        if i.session.is_empty() || !has_heartbeat {
            return None;
        }
        Some(i)
    }
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env_any(keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.trim().is_empty())
}

fn hostname() -> String {
    env_any(&["HOSTNAME", "COMPUTERNAME"])
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".into())
}

fn new_session_id() -> String {
    // RandomState is seeded randomly per process; mixing in the time and
    // pid gives a distinct id without a random-number crate.
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// BlackCAD's lock file for a drawing ([`lock_path_for`] with
/// [`LockKind::BLACKCAD`]).
pub fn lock_path(drawing: &Path) -> PathBuf {
    lock_path_for(drawing, LockKind::BLACKCAD)
}

/// The lock file for a drawing: `part.blc` → `part.bdl` for BlackCAD's
/// kind; any other file gets the extension appended (`part.dwg` →
/// `part.dwg.bdl`) so a DWG and a native file of the same name never share
/// a lock.
pub fn lock_path_for(drawing: &Path, kind: LockKind) -> PathBuf {
    let is_native = drawing
        .extension()
        .map(|e| e.eq_ignore_ascii_case(kind.native))
        .unwrap_or(false);
    if is_native {
        drawing.with_extension(kind.lock)
    } else {
        let mut name = drawing
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".");
        name.push(kind.lock);
        drawing.with_file_name(name)
    }
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::new(ErrorKind::Io, format!("{}: {e}", path.display()))
}

fn read_info(lock: &Path) -> Result<Option<LockInfo>> {
    match std::fs::read_to_string(lock) {
        Ok(t) => Ok(LockInfo::decode(&t).or_else(|| {
            // Unreadable or half-written: date it by the file's mtime, so a
            // lock being created right now is not mistaken for stale.
            let mtime = std::fs::metadata(lock)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(LockInfo {
                user: "unknown".into(),
                session: format!("unreadable-{mtime}"),
                heartbeat: mtime,
                opened: mtime,
                ..Default::default()
            })
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(lock, e)),
    }
}

/// Who has the drawing open, if anyone (stale locks included; check
/// [`LockInfo::is_stale`]).
pub fn presence(drawing: impl AsRef<Path>) -> Result<Option<LockInfo>> {
    presence_for(drawing, LockKind::BLACKCAD)
}

/// [`presence`] for a kind of lock.
pub fn presence_for(drawing: impl AsRef<Path>, kind: LockKind) -> Result<Option<LockInfo>> {
    read_info(&lock_path_for(drawing.as_ref(), kind))
}

/// Result of trying to lock a drawing.
#[derive(Debug)]
pub enum Acquire {
    Acquired(DrawingLock),
    /// Someone else holds a live lock; open read-only.
    HeldBy(LockInfo),
}

/// A held lock. Released (the file removed) on drop.
#[derive(Debug)]
pub struct DrawingLock {
    path: PathBuf,
    info: LockInfo,
    kind: LockKind,
    released: bool,
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    // Closed before the rename. (On WebAssembly, which has no files, `File`
    // is an empty stand-in with nothing to close.)
    #[allow(clippy::drop_non_drop)]
    drop(f);
    std::fs::rename(&tmp, path)
}

fn try_create(path: &Path, info: &LockInfo, kind: LockKind) -> std::io::Result<bool> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            f.write_all(info.encode(kind).as_bytes())?;
            f.sync_all()?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// Move `from` back to `to` without replacing a lock someone created at
/// `to` in the meantime (a hard link fails if `to` exists). Where hard
/// links are unsupported, fall back to a rename.
fn restore_no_clobber(from: &Path, to: &Path) {
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            let _ = std::fs::remove_file(from);
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Someone else holds the path now; leave our copy aside rather
            // than overwrite theirs.
        }
        Err(_) => {
            if !to.exists() {
                let _ = std::fs::rename(from, to);
            }
        }
    }
}

/// Lock a drawing for editing. A stale lock is taken over.
pub fn acquire(drawing: impl AsRef<Path>) -> Result<Acquire> {
    acquire_as(drawing, user_name())
}

/// The signed-in user's name as [`acquire`] records it: `USER` or
/// `USERNAME` from the environment, or "unknown".
pub fn user_name() -> String {
    env_any(&["USER", "USERNAME"]).unwrap_or_else(|| "unknown".into())
}

/// [`acquire`] with an explicit user name (for apps with their own sign-in).
/// The lock names this crate as the program; [`acquire_with`] names another.
pub fn acquire_as(drawing: impl AsRef<Path>, user: String) -> Result<Acquire> {
    acquire_with(drawing, user, &format!("cad-native {}", crate::VERSION))
}

/// [`acquire_as`] naming the program that holds the lock ("name version"),
/// which other programs show to people who find the drawing locked.
pub fn acquire_with(drawing: impl AsRef<Path>, user: String, app: &str) -> Result<Acquire> {
    acquire_for(drawing, user, app, LockKind::BLACKCAD)
}

/// [`acquire_with`] for a kind of lock (CADCraft's `.cdl` beside `.cad`).
pub fn acquire_for(drawing: impl AsRef<Path>, user: String, app: &str, kind: LockKind) -> Result<Acquire> {
    let path = lock_path_for(drawing.as_ref(), kind);
    let now = now_unix();
    let info = LockInfo {
        user,
        host: hostname(),
        pid: std::process::id(),
        app: app.to_string(),
        session: new_session_id(),
        opened: now,
        heartbeat: now,
    };
    for _ in 0..3 {
        if try_create(&path, &info, kind).map_err(|e| io(&path, e))? {
            return Ok(Acquire::Acquired(DrawingLock {
                path,
                info,
                kind,
                released: false,
            }));
        }
        let Some(existing) = read_info(&path)? else {
            continue; // released between our two calls; try again
        };
        if !existing.is_stale(now) {
            return Ok(Acquire::HeldBy(existing));
        }
        // Take over: move the stale lock aside, then confirm we moved the
        // lock we judged stale and not one created a moment ago.
        let mut aside = path.as_os_str().to_os_string();
        aside.push(format!(".stale-{}", info.session));
        let aside = PathBuf::from(aside);
        if std::fs::rename(&path, &aside).is_ok() {
            // Another process may have replaced the stale lock between our
            // read and the rename; if what we moved is not the stale lock we
            // judged, or is fresh, put it back.
            let moved = read_info(&aside).ok().flatten();
            let wrong = moved
                .as_ref()
                .map(|m| m.session != existing.session || !m.is_stale(now_unix()))
                .unwrap_or(false);
            if wrong {
                restore_no_clobber(&aside, &path);
                if let Some(m) = moved {
                    return Ok(Acquire::HeldBy(m));
                }
            }
            let _ = std::fs::remove_file(&aside);
        }
    }
    match read_info(&path)? {
        Some(i) => Ok(Acquire::HeldBy(i)),
        None => Err(Error::new(
            ErrorKind::Io,
            format!("{}: could not lock", path.display()),
        )),
    }
}

impl DrawingLock {
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Update the heartbeat. Fails if the lock was taken over (because this
    /// process stopped refreshing it) or removed; the app should then stop
    /// saving and reopen read-only.
    pub fn refresh(&mut self) -> Result<()> {
        match read_info(&self.path)? {
            Some(cur) if cur.session == self.info.session => {}
            Some(other) => {
                return Err(Error::new(
                    ErrorKind::Io,
                    format!(
                        "lock taken over by {}@{} (the heartbeat lapsed)",
                        other.user, other.host
                    ),
                ))
            }
            None => {
                return Err(Error::new(
                    ErrorKind::Io,
                    format!("{}: lock file was removed", self.path.display()),
                ))
            }
        }
        self.info.heartbeat = now_unix();
        write_atomic(&self.path, &self.info.encode(self.kind)).map_err(|e| io(&self.path, e))
    }

    /// Remove the lock now.
    pub fn release(mut self) -> Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<()> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        match read_info(&self.path)? {
            Some(cur) if cur.session == self.info.session => {
                std::fs::remove_file(&self.path).map_err(|e| io(&self.path, e))
            }
            _ => Ok(()), // not ours any more; leave it
        }
    }
}

impl Drop for DrawingLock {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cad-lock-{tag}-{}", new_session_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn paths() {
        assert_eq!(lock_path(Path::new("/x/part.blc")), Path::new("/x/part.bdl"));
        assert_eq!(lock_path(Path::new("/x/part.BLC")), Path::new("/x/part.bdl"));
        assert_eq!(lock_path(Path::new("/x/part.dwg")), Path::new("/x/part.dwg.bdl"));
        // A CADCraft drawing is not BlackCAD's own: its name is kept whole.
        assert_eq!(lock_path(Path::new("/x/part.cad")), Path::new("/x/part.cad.bdl"));
        // CADCraft's kind pairs .cdl with .cad.
        let c = LockKind::CADCRAFT;
        assert_eq!(lock_path_for(Path::new("/x/part.cad"), c), Path::new("/x/part.cdl"));
        assert_eq!(lock_path_for(Path::new("/x/part.dwg"), c), Path::new("/x/part.dwg.cdl"));
    }

    #[test]
    fn each_kind_writes_its_own_name_and_reads_both() {
        let dir = tmpdir("kinds");
        let d = dir.join("d.cad");
        let Acquire::Acquired(lock) = acquire_for(&d, "alice".into(), "cadcraft 0.10.0", LockKind::CADCRAFT).unwrap() else {
            panic!("not acquired");
        };
        let text = std::fs::read_to_string(dir.join("d.cdl")).unwrap();
        assert!(text.starts_with("cdl 1\n"), "{text}");
        assert_eq!(presence_for(&d, LockKind::CADCRAFT).unwrap().unwrap().user, "alice");
        drop(lock);
        let b = dir.join("b.blc");
        let Acquire::Acquired(_lock) = acquire_as(&b, "bob".into()).unwrap() else { panic!("not acquired") };
        assert!(std::fs::read_to_string(dir.join("b.bdl")).unwrap().starts_with("bdl 1\n"));
        // A lock written under the other name still reads.
        std::fs::write(dir.join("c.bdl"), "cdl 1\nuser=carol\nsession=s\nheartbeat=1\n").unwrap();
        assert_eq!(presence(dir.join("c.blc")).unwrap().unwrap().user, "carol");
    }

    #[test]
    fn acquire_hold_release() {
        let dir = tmpdir("a");
        let dwg = dir.join("d.blc");
        let Acquire::Acquired(mut lock) = acquire_as(&dwg, "alice".into()).unwrap() else {
            panic!("first acquire should succeed");
        };
        match acquire_as(&dwg, "bob".into()).unwrap() {
            Acquire::HeldBy(i) => assert_eq!(i.user, "alice"),
            Acquire::Acquired(_) => panic!("second acquire must not succeed"),
        }
        assert_eq!(
            presence(&dwg).unwrap().unwrap().session,
            lock.info().session
        );
        lock.refresh().unwrap();
        lock.release().unwrap();
        assert!(presence(&dwg).unwrap().is_none());
        let Acquire::Acquired(_l) = acquire_as(&dwg, "bob".into()).unwrap() else {
            panic!("acquire after release should succeed");
        };
        drop(_l);
        assert!(presence(&dwg).unwrap().is_none(), "drop releases");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_lock_is_taken_over() {
        let dir = tmpdir("b");
        let dwg = dir.join("d.blc");
        let old = LockInfo {
            user: "crashed".into(),
            host: "h".into(),
            session: "old".into(),
            heartbeat: now_unix() - STALE_AFTER_SECS - 10,
            ..Default::default()
        };
        std::fs::write(lock_path(&dwg), old.encode(LockKind::BLACKCAD)).unwrap();
        let Acquire::Acquired(mut lock) = acquire_as(&dwg, "alice".into()).unwrap() else {
            panic!("stale lock should be taken over");
        };
        assert_eq!(presence(&dwg).unwrap().unwrap().user, "alice");
        // Simulate someone else taking over after our heartbeat lapsed.
        let mut other = lock.info().clone();
        other.session = "someone-else".into();
        other.user = "bob".into();
        std::fs::write(lock_path(&dwg), other.encode(LockKind::BLACKCAD)).unwrap();
        assert!(lock.refresh().is_err());
        drop(lock);
        assert_eq!(
            presence(&dwg).unwrap().unwrap().user,
            "bob",
            "drop leaves others' lock"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_cut_short_counts_as_fresh_by_mtime() {
        let dir = tmpdir("d");
        let dwg = dir.join("d.blc");
        // Written up to the session line, then interrupted.
        std::fs::write(lock_path(&dwg), "bdl 1\nuser=a\nsession=abc\n").unwrap();
        assert!(matches!(
            acquire_as(&dwg, "b".into()).unwrap(),
            Acquire::HeldBy(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_lock_counts_as_fresh_by_mtime() {
        let dir = tmpdir("c");
        let dwg = dir.join("d.blc");
        std::fs::write(lock_path(&dwg), "half-writ").unwrap();
        assert!(matches!(
            acquire_as(&dwg, "a".into()).unwrap(),
            Acquire::HeldBy(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
