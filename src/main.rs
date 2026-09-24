use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use jaq_json::Val;
use jaq_json::write::{Pp, Styles};

use dq::Query;

/// Query directory trees with jq filters.
///
/// The filter's input is the entry for PATH. Use `tree` to walk below it,
/// `ls` for its children and `content` to read a file.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// jq filter to run
    filter: String,
    /// Directory or file to start from
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Print strings without quotes
    #[arg(short, long)]
    raw_output: bool,
    /// Print each value on a single line
    #[arg(short, long)]
    compact_output: bool,
    /// Never color the output
    #[arg(short = 'M', long)]
    monochrome_output: bool,
}

// Exit codes follow jq where the meaning overlaps.
const EXIT_USAGE: u8 = 2;
const EXIT_COMPILE: u8 = 3;
const EXIT_RUNTIME: u8 = 5;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            eprintln!("dq: {msg}");
            ExitCode::from(code)
        }
    }
}

fn run(cli: &Cli) -> Result<(), (u8, String)> {
    let query = Query::compile(&cli.filter).map_err(|e| (EXIT_COMPILE, e))?;
    let root =
        dq::root(&cli.path).map_err(|e| (EXIT_USAGE, format!("{}: {e}", cli.path.display())))?;

    let stdout = io::stdout();
    let color =
        !cli.monochrome_output && stdout.is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let pp = Pp {
        indent: (!cli.compact_output).then(|| "  ".to_owned()),
        sep_space: !cli.compact_output,
        styles: if color {
            Styles::ansi()
        } else {
            Styles::default()
        },
        ..Pp::default()
    };
    let mut out = io::BufWriter::new(stdout.lock());
    let io_err = |e: io::Error| (EXIT_RUNTIME, e.to_string());

    for v in query.run(root) {
        let v = v.map_err(|e| (EXIT_RUNTIME, format!("error: {}", error_msg(e))))?;
        print_val(&mut out, &pp, cli.raw_output, &v).map_err(io_err)?;
    }
    out.flush().map_err(io_err)
}

fn print_val(w: &mut impl Write, pp: &Pp, raw: bool, v: &Val) -> io::Result<()> {
    match v {
        Val::TStr(s) | Val::BStr(s) if raw => w.write_all(s)?,
        _ => jaq_json::write::write(w, pp, 0, v)?,
    }
    writeln!(w)
}

/// Error text, without quotes when the error is a plain string.
fn error_msg(e: dq::Error<Val>) -> String {
    match e.into_val() {
        Val::TStr(s) => String::from_utf8_lossy(&s).into_owned(),
        v => v.to_string(),
    }
}
