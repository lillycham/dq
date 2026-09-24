//! Filesystem entries as jq values.
//!
//! An entry is a plain object of metadata, built from a single `lstat`.
//! Children and file contents are never embedded; they are fetched on demand
//! by the `ls` and `content` filters, so walking a large tree stays cheap.

use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use jaq_json::{Map, Val};

/// Build the entry object for `path` without following symlinks.
pub fn stat(path: &Path) -> io::Result<Val> {
    let meta = fs::symlink_metadata(path)?;
    Ok(from_metadata(path, &meta))
}

/// Entries directly inside `dir`, sorted by name.
///
/// Anything that is not a directory (including a symlink to one) has no children,
/// which keeps recursive walks finite.
pub fn children(dir: &Path) -> io::Result<Vec<Val>> {
    if !fs::symlink_metadata(dir)?.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<_> = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    names.sort();
    names
        .iter()
        .map(|name| stat(&join(dir, Path::new(name))))
        .collect()
}

/// Join `rel` onto `base`, dropping a leading `./` so paths print like `find`'s without the prefix.
pub fn join(base: &Path, rel: &Path) -> PathBuf {
    if base == Path::new(".") {
        rel.to_path_buf()
    } else {
        base.join(rel)
    }
}

fn from_metadata(path: &Path, meta: &Metadata) -> Val {
    let ft = meta.file_type();
    let typ = if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    };
    // `.` and `..` have no file name of their own, so take it from the resolved path.
    let resolved = path
        .file_name()
        .is_none()
        .then(|| fs::canonicalize(path).ok())
        .flatten();
    let name = resolved
        .as_deref()
        .unwrap_or(path)
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy());
    // Directories have no meaningful extension, and dotfiles like `.bashrc` have none either.
    let (stem, ext) = match typ {
        "dir" => (Some(name.to_string()), None),
        _ => (
            Path::new(&*name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned()),
            Path::new(&*name)
                .extension()
                .map(|s| s.to_string_lossy().into_owned()),
        ),
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64());
    let target = ft
        .is_symlink()
        .then(|| fs::read_link(path).ok())
        .flatten()
        .map(|t| t.to_string_lossy().into_owned());

    let mut m = Map::default();
    let mut set = |k: &str, v: Val| {
        m.insert(Val::from(k.to_owned()), v);
    };
    set("name", Val::from(name.into_owned()));
    set("path", Val::from(path.to_string_lossy().into_owned()));
    set("type", Val::from(typ.to_owned()));
    set("size", Val::from(meta.len() as usize));
    set("stem", opt_str(stem));
    set("ext", opt_str(ext));
    set("hidden", Val::from(is_hidden(path)));
    set("mtime", mtime.map_or(Val::Null, Val::from));
    set("mode", Val::from(mode(meta)));
    set("target", opt_str(target));
    Val::obj(m)
}

fn opt_str(s: Option<String>) -> Val {
    s.map_or(Val::Null, Val::from)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.') && n != "." && n != "..")
}

#[cfg(unix)]
fn mode(meta: &Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:04o}", meta.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode(meta: &Metadata) -> String {
    if meta.permissions().readonly() {
        "0444"
    } else {
        "0644"
    }
    .to_owned()
}
