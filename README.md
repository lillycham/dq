# dq

[![CI](https://github.com/lillycham/dq/actions/workflows/ci.yml/badge.svg)](https://github.com/lillycham/dq/actions/workflows/ci.yml)

`jq` for directories. dq runs [jq](https://jqlang.org) filters over a directory tree:
entries are objects, walks are streams, and file contents lead straight back into JSON.

```sh
dq -r 'files | select(.ext == "rs") | .path'           # like find -name '*.rs'
dq '[files | .size] | add'                             # total size in bytes
dq 'at("package.json") | content | fromjson | .version'
dq -r 'tree(.name != ".git" and .name != "target") | .path'
```

The language is full jq, provided by [jaq](https://github.com/01mf02/jaq), plus the filters below.
See [docs/language.md](docs/language.md) for the full reference.

## Usage

```
dq [OPTIONS] <FILTER> [PATH]
```

The filter's input is the entry for `PATH` (default `.`).

| Option | Meaning |
| --- | --- |
| `-r`, `--raw-output` | Print strings without quotes |
| `-c`, `--compact-output` | Print each value on one line |
| `-M`, `--monochrome-output` | Never color the output |
| `--apply` | Run the planned filesystem changes |

## Entries

An entry is an object with these fields:

| Field | Example | Notes |
| --- | --- | --- |
| `name` | `"main.rs"` | |
| `path` | `"src/main.rs"` | Relative to `PATH` as given |
| `type` | `"file"` | `file`, `dir`, `symlink` or `other` |
| `size` | `1234` | Bytes |
| `stem`, `ext` | `"main"`, `"rs"` | `ext` is `null` for directories and files without one |
| `hidden` | `false` | Name starts with `.` |
| `mtime` | `1790222650.47` | Seconds since the Unix epoch |
| `mode` | `"0644"` | Permission bits |
| `target` | `"../lib"` | Symlink target, else `null` |

Entries hold metadata only. Children and contents load when a filter asks for them.
Symlinks are never followed, so walks always finish.

## Filters

| Filter | Output |
| --- | --- |
| `ls` | Direct children, sorted by name |
| `tree` | All entries below the input, depth first |
| `tree(f)` | Like `tree`, but skips entries where `f` is false and does not descend into them |
| `files`, `dirs` | `tree`, limited to files or directories |
| `at($path)` | The entry at `$path` below the input |
| `child($name)` | Path of `$name` below the input, which need not exist |
| `stat` | The entry for a path string |
| `content` | File contents as a string |

## Changing files

Actions do not change anything while the filter runs.
They output operations, and dq collects these into a plan:

| Action | Effect |
| --- | --- |
| `rm` | Remove the entry (directories recursively) |
| `mv($to)` | Move or rename; `$to` is relative to the entry's directory |
| `cp($to)` | Copy (directories recursively); `$to` as for `mv` |
| `write($s)` | Write `$s` to the file, creating or replacing it |
| `mkdir` | Create the directory and any missing parents |

Actions take an entry or a path string.
Plain strings are relative to the working directory, so use `child` to name new paths:

```sh
dq 'files | select(.ext == "jpeg") | mv(.stem + ".jpg")'          # prints the plan
dq --apply 'files | select(.ext == "jpeg") | mv(.stem + ".jpg")'  # runs it
dq --apply '(child("notes") | mkdir), (child("notes/todo.md") | write("- [ ] \n"))'
```

dq checks the whole plan before it runs anything.
It rejects a plan that removes a path and also uses something inside it,
creates the same path twice, or overwrites an existing entry with `mv` or `cp`.

## Development

```sh
nix develop    # shell with cargo, clippy, rustfmt, rust-analyzer and cargo-deny
cargo test
cargo deny check licenses   # dependency license policy, see deny.toml
nix build      # also runs the tests
```

## License

dq
Copyright (C) 2026  Lilly Cham

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
The full text is in [LICENSE](LICENSE).
