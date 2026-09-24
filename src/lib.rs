//! dq: query and change directory trees with jq filters.
//!
//! A query runs a jq filter on the entry for a root directory.
//! See `README.md` for the language that dq adds on top of jq.

pub mod entry;
pub mod funs;
pub mod plan;

use std::path::Path;

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Ctx, Vars, compile, load, unwrap_valr};
use jaq_json::Val;

pub use jaq_core::Error;
pub use plan::{Op, Plan};

/// Compiled dq filter.
pub struct Query {
    filter: jaq_core::Filter<funs::D>,
}

impl Query {
    /// Parse and compile `code`, returning a readable message on failure.
    pub fn compile(code: &str) -> Result<Self, String> {
        let defs = jaq_core::defs()
            .chain(jaq_std::defs())
            .chain(jaq_json::defs())
            .chain(funs::defs());
        let natives = jaq_core::funs()
            .chain(jaq_std::funs())
            .chain(jaq_json::funs())
            .chain(funs::funs());

        let arena = Arena::default();
        let modules = Loader::new(defs)
            .load(&arena, File { code, path: () })
            .map_err(load_errors)?;
        let filter = jaq_core::Compiler::default()
            .with_funs(natives)
            .compile(modules)
            .map_err(compile_errors)?;
        Ok(Self { filter })
    }

    /// Run the filter on `input`, yielding its outputs lazily.
    pub fn run(&self, input: Val) -> impl Iterator<Item = Result<Val, Error<Val>>> + '_ {
        let ctx = Ctx::<funs::D>::new(&self.filter.lut, Vars::new([]));
        self.filter.id.run((ctx, input)).map(unwrap_valr)
    }
}

/// Entry object for the root of a query.
pub fn root(path: &Path) -> std::io::Result<Val> {
    entry::stat(path)
}

fn load_errors(errs: load::Errors<&str, ()>) -> String {
    let mut out = Vec::new();
    for (file, err) in errs {
        match err {
            load::Error::Io(es) => {
                out.extend(
                    es.into_iter()
                        .map(|(path, e)| format!("cannot load {path}: {e}")),
                );
            }
            load::Error::Lex(es) => {
                for (expect, rest) in es {
                    out.push(format!(
                        "{}: expected {}",
                        pos(file.code, rest),
                        expect.as_str()
                    ));
                }
            }
            load::Error::Parse(es) => {
                for (expect, found) in es {
                    let what = match found {
                        "" => "end of input".to_owned(),
                        s => format!("`{s}`"),
                    };
                    out.push(format!(
                        "{}: expected {}, found {what}",
                        pos(file.code, found),
                        expect.as_str()
                    ));
                }
            }
        }
    }
    out.join("\n")
}

fn compile_errors(errs: compile::Errors<&str, ()>) -> String {
    let mut out = Vec::new();
    for (file, es) in errs {
        for (name, undefined) in es {
            let arity = match undefined {
                compile::Undefined::Filter(n) => format!("/{n}"),
                _ => String::new(),
            };
            out.push(format!(
                "{}: undefined {} `{name}{arity}`",
                pos(file.code, name),
                undefined.as_str()
            ));
        }
    }
    out.join("\n")
}

/// Line and column of `part` inside `code`, where `part` is a slice of `code`.
fn pos(code: &str, part: &str) -> String {
    let offset = (part.as_ptr() as usize).wrapping_sub(code.as_ptr() as usize);
    let Some(before) = code.get(..offset) else {
        return "filter".to_owned();
    };
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    format!("{line}:{col}")
}
