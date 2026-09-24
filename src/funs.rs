//! Native filters and jq definitions that make up dq's built-in library.

use std::io;
use std::path::{Path, PathBuf};

use jaq_core::box_iter::{BoxIter, box_once};
use jaq_core::load::parse::Def;
use jaq_core::native::{Filter, Fun, bome, v};
use jaq_core::{Cv, DataT, Error, Exn, RunPtr, ValX, data};
use jaq_json::Val;
use jaq_std::ValT as _;

use crate::entry;

/// Data kind that dq filters run on.
pub type D = data::JustLut<Val>;

/// Definitions written in jq, such as `tree` and the plan actions.
pub fn defs() -> impl Iterator<Item = Def<&'static str>> {
    jaq_core::load::parse(include_str!("defs.jq"), |p| p.defs())
        .expect("dq's built-in definitions must parse")
        .into_iter()
}

/// Filters implemented in Rust.
pub fn funs<D: for<'a> DataT<V<'a> = Val>>() -> impl Iterator<Item = Fun<D>> {
    base().into_vec().into_iter().map(jaq_core::native::run)
}

fn base<D: for<'a> DataT<V<'a> = Val>>() -> Box<[Filter<RunPtr<D>>]> {
    Box::new([
        ("ls", v(0), |cv| {
            match path_of(&cv.1).and_then(|p| entry::children(&p).map_err(|e| io_err(&p, e))) {
                Ok(xs) => Box::new(xs.into_iter().map(Ok)) as BoxIter<_>,
                Err(e) => box_once(Err(Exn::from(e))),
            }
        }),
        ("stat", v(0), |cv| {
            bome(path_of(&cv.1).and_then(|p| stat(&p)))
        }),
        ("at", v(1), |cv| at(cv)),
        ("content", v(0), |cv| {
            let read = |p: PathBuf| std::fs::read(&p).map_err(|e| io_err(&p, e));
            bome(path_of(&cv.1).and_then(read).map(Val::utf8_str))
        }),
    ])
}

fn at<'a, D: for<'b> DataT<V<'b> = Val>>(mut cv: Cv<'a, D>) -> BoxIter<'a, ValX<'a, Val>> {
    let rel = cv.0.pop_var();
    let path = path_of(&cv.1).and_then(|base| Ok(entry::join(&base, &str_of(&rel)?)));
    bome(path.and_then(|p| stat(&p)))
}

fn stat(path: &Path) -> Result<Val, Error<Val>> {
    entry::stat(path).map_err(|e| io_err(path, e))
}

/// Path of an entry object, or the string itself if the value is a string.
fn path_of(v: &Val) -> Result<PathBuf, Error<Val>> {
    match v {
        Val::Obj(m) => match m.get(&Val::from("path".to_owned())) {
            Some(p) => str_of(p),
            None => Err(Error::str(format_args!("{v} is not a filesystem entry"))),
        },
        _ => str_of(v),
    }
}

fn str_of(v: &Val) -> Result<PathBuf, Error<Val>> {
    let bytes = v.try_as_utf8_bytes()?;
    Ok(PathBuf::from(String::from_utf8_lossy(bytes).into_owned()))
}

fn io_err(path: &Path, e: io::Error) -> Error<Val> {
    Error::str(format_args!("{}: {e}", path.display()))
}
