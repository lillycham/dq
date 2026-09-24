//! Filesystem changes requested by a query.
//!
//! Filters never change the filesystem while they run. Actions such as `rm` yield
//! operation objects, which dq collects into a [`Plan`]. The whole plan is checked
//! before anything runs, so a conflicting or impossible plan changes nothing.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use jaq_json::Val;
use jaq_std::ValT as _;

/// Key that marks an object as an operation rather than an ordinary value.
const OP_KEY: &str = "dq:op";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Rm(PathBuf),
    Mv { from: PathBuf, to: PathBuf },
    Cp { from: PathBuf, to: PathBuf },
    Write { path: PathBuf, content: Vec<u8> },
    Mkdir(PathBuf),
}

impl Op {
    /// Read an operation from a filter output.
    ///
    /// Returns `None` if the value is not an operation at all,
    /// and `Some(Err(_))` if it is one but is malformed.
    pub fn from_val(v: &Val) -> Option<Result<Self, String>> {
        let Val::Obj(m) = v else { return None };
        let get = |k: &str| m.get(&Val::from(k.to_owned()));
        let kind = get(OP_KEY)?;
        let str_field = |k: &str| -> Result<Vec<u8>, String> {
            get(k)
                .and_then(|v| v.as_bytes())
                .map(<[u8]>::to_vec)
                .ok_or_else(|| format!("operation {v} needs a string `{k}`"))
        };
        let path_field =
            |k: &str| str_field(k).map(|b| normalize(Path::new(&*String::from_utf8_lossy(&b))));
        let op = (|| {
            let path = path_field("path")?;
            // Targets of `mv` and `cp` are relative to the directory that holds the source.
            let target = || {
                path_field("to")
                    .map(|to| normalize(&path.parent().unwrap_or(Path::new("")).join(to)))
            };
            Ok(match kind.as_bytes() {
                Some(b"rm") => Self::Rm(path),
                Some(b"mv") => Self::Mv {
                    to: target()?,
                    from: path,
                },
                Some(b"cp") => Self::Cp {
                    to: target()?,
                    from: path,
                },
                Some(b"write") => Self::Write {
                    content: str_field("content")?,
                    path,
                },
                Some(b"mkdir") => Self::Mkdir(path),
                _ => return Err(format!("unknown operation {kind}")),
            })
        })();
        Some(op)
    }

    /// Path whose whole subtree this operation removes, if any.
    fn removes(&self) -> Option<&Path> {
        match self {
            Self::Rm(p) | Self::Mv { from: p, .. } => Some(p),
            _ => None,
        }
    }

    /// Path this operation creates or overwrites, if any.
    fn creates(&self) -> Option<&Path> {
        match self {
            Self::Mv { to, .. } | Self::Cp { to, .. } => Some(to),
            Self::Write { path, .. } | Self::Mkdir(path) => Some(path),
            Self::Rm(_) => None,
        }
    }

    /// Every path this operation reads or changes.
    fn paths(&self) -> Vec<&Path> {
        match self {
            Self::Rm(p) | Self::Mkdir(p) | Self::Write { path: p, .. } => vec![p],
            Self::Mv { from, to } | Self::Cp { from, to } => vec![from, to],
        }
    }

    fn apply(&self) -> io::Result<()> {
        match self {
            Self::Rm(p) if fs::symlink_metadata(p)?.is_dir() => fs::remove_dir_all(p),
            Self::Rm(p) => fs::remove_file(p),
            Self::Mv { from, to } => fs::rename(from, to),
            Self::Cp { from, to } => copy(from, to),
            Self::Write { path, content } => fs::write(path, content),
            Self::Mkdir(p) => fs::create_dir_all(p),
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Rm(p) => write!(f, "rm     {}", p.display()),
            Self::Mv { from, to } => write!(f, "mv     {} -> {}", from.display(), to.display()),
            Self::Cp { from, to } => write!(f, "cp     {} -> {}", from.display(), to.display()),
            Self::Write { path, content } => {
                write!(f, "write  {} ({} bytes)", path.display(), content.len())
            }
            Self::Mkdir(p) => write!(f, "mkdir  {}", p.display()),
        }
    }
}

/// Ordered list of operations.
#[derive(Debug, Default)]
pub struct Plan {
    ops: Vec<Op>,
    seen: HashSet<Op>,
}

impl Plan {
    /// Add an operation. Exact duplicates are dropped.
    pub fn push(&mut self, op: Op) {
        if self.seen.insert(op.clone()) {
            self.ops.push(op);
        }
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Check the plan against itself and the current filesystem.
    ///
    /// Returns every problem found, so the user can fix them all at once.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        let exists = |p: &Path| fs::symlink_metadata(p).ok();
        let mkdirs: Vec<&Path> = self
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Mkdir(p) => Some(p.as_path()),
                _ => None,
            })
            .collect();

        for (i, op) in self.ops.iter().enumerate() {
            if let Op::Rm(src) | Op::Mv { from: src, .. } | Op::Cp { from: src, .. } = op
                && exists(src).is_none()
            {
                errs.push(format!("{op}: {} does not exist", src.display()));
            }

            if let Some(dst) = op.creates() {
                match (op, exists(dst)) {
                    (Op::Mkdir(_), Some(m)) if m.is_dir() => {}
                    (Op::Write { .. }, Some(m)) if !m.is_dir() => {}
                    (_, Some(_)) => errs.push(format!("{op}: {} already exists", dst.display())),
                    (_, None) => {
                        let parent = dst.parent().filter(|p| !p.as_os_str().is_empty());
                        let planned = |p: &Path| mkdirs.iter().any(|m| m.starts_with(p));
                        if let Some(parent) = parent.filter(|p| exists(p).is_none() && !planned(p))
                        {
                            errs.push(format!(
                                "{op}: directory {} does not exist",
                                parent.display()
                            ));
                        }
                    }
                }
            }

            for other in &self.ops[i + 1..] {
                if let (Some(a), Some(b)) = (op.creates(), other.creates())
                    && a == b
                    && !matches!((op, other), (Op::Mkdir(_), Op::Mkdir(_)))
                {
                    errs.push(format!("{op} and {other} both create {}", a.display()));
                }
                if conflicts(op, other) || conflicts(other, op) {
                    errs.push(format!("{op} conflicts with {other}"));
                }
            }
        }
        errs.dedup();
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// Run every operation in order, stopping at the first failure.
    pub fn apply(&self) -> Result<(), (usize, &Op, io::Error)> {
        for (i, op) in self.ops.iter().enumerate() {
            op.apply().map_err(|e| (i, op, e))?;
        }
        Ok(())
    }
}

/// Whether `x` removes a subtree that `y` also uses.
fn conflicts(x: &Op, y: &Op) -> bool {
    let Some(gone) = x.removes() else {
        return false;
    };
    y.paths().into_iter().any(|p| {
        // Creating a parent directory of a removed path is harmless.
        let harmless = matches!(y, Op::Mkdir(_)) && gone.starts_with(p) && gone != p;
        (p.starts_with(gone) || gone.starts_with(p)) && !harmless
    })
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.is_dir() {
        fs::create_dir(to)?;
        for e in fs::read_dir(from)? {
            let e = e?;
            copy(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else if meta.is_symlink() {
        copy_symlink(&fs::read_link(from)?, to)
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

#[cfg(unix)]
fn copy_symlink(target: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, to)
}

#[cfg(not(unix))]
fn copy_symlink(_target: &Path, to: &Path) -> io::Result<()> {
    Err(io::Error::other(format!(
        "{}: copying symlinks is not supported on this platform",
        to.display()
    )))
}

/// Resolve `.` and `..` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            c => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use std::path::Path;

    #[test]
    fn normalize_paths() {
        let n = |s| normalize(Path::new(s));
        assert_eq!(n("a/./b/../c"), Path::new("a/c"));
        assert_eq!(n("a/.."), Path::new("."));
        assert_eq!(n("../a"), Path::new("../a"));
        assert_eq!(n("/.."), Path::new("/"));
    }
}
