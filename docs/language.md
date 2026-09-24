# The dq language

A dq filter is a jq program. Everything jq can do, dq can do: pipes, `select`,
`map`, object construction, string interpolation, `reduce`, user-defined
functions, and so on. dq adds a small library on top of that for reading
directory trees and planning changes to them.

This document covers the parts that are specific to dq. For the language itself:

- The [jq manual](https://jqlang.org/manual/) is the best introduction to the syntax and the
  standard library.
- dq runs filters with [jaq](https://github.com/01mf02/jaq), a Rust implementation of jq.
  Its README lists the [places where jaq differs from jq](https://github.com/01mf02/jaq#differences-between-jq-and-jaq).
  These are mostly small edge cases.

## Contents

- [How a query runs](#how-a-query-runs)
- [Entries](#entries)
- [Reading the tree](#reading-the-tree)
- [Changing the tree](#changing-the-tree)
- [Paths](#paths)
- [Command line](#command-line)
- [Pitfalls](#pitfalls)

## How a query runs

```
dq [OPTIONS] <FILTER> [PATH]
```

dq builds the [entry](#entries) for `PATH` (default `.`) and gives it to the
filter as its input `.`. It then prints each output value as JSON, the way jq does.

```sh
dq '.name'            # name of the current directory
dq '.type' Cargo.toml # "file"
```

The filter doesn't get the whole tree up front. It gets one entry, and uses
filters such as [`ls`](#ls) and [`tree`](#tree) to move through the tree from there.

## Entries

An entry is a plain jq object that describes one file, directory or other
filesystem object. dq builds it from a single `lstat` call.

| Field | Type | Description |
| --- | --- | --- |
| `name` | string | Final path component, such as `"main.rs"`. For `.` and `..` this is the real directory name. |
| `path` | string | Path of the entry. See [Paths](#paths). |
| `type` | string | `"file"`, `"dir"`, `"symlink"` or `"other"` (sockets, devices, FIFOs). |
| `size` | number | Size in bytes. For directories and symlinks, the value depends on the platform. |
| `stem` | string or null | `name` without its extension: `"main"` for `main.rs`. For a directory, the full name. |
| `ext` | string or null | Extension without the dot: `"rs"`. `null` for directories, dotfiles such as `.bashrc`, and names with no extension. |
| `hidden` | boolean | `true` if `name` starts with `.`. |
| `mtime` | number or null | Last change time in seconds since the Unix epoch, with a fractional part. |
| `mode` | string | Permission bits as four octal digits, such as `"0644"`. |
| `target` | string or null | For a symlink, the path it points to, as stored in the link. Otherwise `null`. |

Entries are ordinary values. You can index them, build new objects from them,
and compare them like any other jq value:

```sh
dq -c 'files | {path, kb: (.size / 1024 | floor)}'
dq -r 'files | select(.mtime > now - 86400) | .path'   # changed in the last day
dq -r 'files | select(.mode == "0755") | .path'        # executables
```

An entry has **no children and no contents**. That keeps walks cheap: dq
reads a directory only when `ls` asks for it, and a file only when `content`
asks for it.

**dq never follows symlinks.** A symlink is reported as `"type": "symlink"`
with its `target`, and walks do not descend into it. This means a walk always
finishes, even when there are symlink loops.

## Reading the tree

These filters accept an entry as input. Most also accept a path string; see
the notes for each one.

### `ls`

Outputs the entries directly inside the input directory, sorted by name.

```sh
dq -r 'ls | .name'
dq -r 'at("src") | ls | .path'
```

- The sort compares raw bytes, so `README.md` comes before `data.json`.
- For a file or a symlink, `ls` outputs nothing. That is not an error.
- If the input does not exist or cannot be read, `ls` raises an error.
- The input can be an entry or a path string.

### `tree`

Outputs every entry below the input, depth first, with each directory
before its contents. The input itself is not included.

```sh
dq -r 'tree | .path'
```

To include the input, write `., tree`.

`tree` is lazy. It reads each directory only when the walk gets there, so
`first(tree | select(.name == "Cargo.toml"))` stops at the first match.

### `tree(f)`

Like `tree`, but it outputs and descends into only the entries for which `f`
is true. Entries that fail `f` are skipped along with everything below them,
so this is the way to leave out large directories:

```sh
dq -r 'tree(.name != ".git" and .name != "target") | .path'
dq -r 'tree(.hidden | not) | .path'
```

Compare this with `tree | select(f)`, which still walks into the skipped
directories and only leaves them out of the output.

### `files` and `dirs`

Short forms of `tree | select(.type == "file")` and
`tree | select(.type == "dir")`.

```sh
dq '[files | .size] | add'   # total size of all files
dq '[dirs] | length'         # number of directories
```

To leave out some directories as well, use `tree(f)` with a `select`:

```sh
dq -r 'tree(.name != "node_modules") | select(.type == "file") | .path'
```

### `at($path)`

Outputs the entry at `$path`, relative to the input.

```sh
dq 'at("src/main.rs")'
dq 'at("src") | at("lib.rs")'
```

- If nothing exists at that path, `at` raises an error.
- `$path` may contain `..`, and it may be absolute.
- The input can be an entry or a path string.

### `stat`

Takes a path string and outputs its entry. Relative paths are resolved
against the working directory, not against `PATH`; see [Paths](#paths).

```sh
dq '"Cargo.toml" | stat | .size'
```

Usually `at` is the better choice, because it is relative to an entry you already have.

### `content`

Outputs the contents of a file as a string.

```sh
dq -r 'at("Cargo.toml") | content'
dq 'at("package.json") | content | fromjson | .dependencies | keys'
dq -c 'files | select(.ext == "rs") | {path, lines: (content | split("\n") | length)}'
```

- `content` reads the whole file into memory.
- Invalid UTF-8 is kept as is. The bytes pass through unchanged, but string
  functions may treat them as replacement characters.
- On a directory, or on a path that can't be read, `content` raises an error.
- The input can be an entry or a path string.

Because file contents are ordinary strings, all of jq's string, regular
expression and JSON functions work on them. That includes `test`, `split`,
`fromjson` and `@base64d`.

### `child($name)`

Outputs the path of `$name` inside the input, as a string. The path doesn't
need to exist, so `child` is how you name new files and directories for the
[actions](#changing-the-tree).

```sh
dq 'child("build")'                  # "build"
dq 'at("src") | child("new.rs")'     # "src/new.rs"
```

## Changing the tree

The actions `rm`, `mv`, `cp`, `write` and `mkdir` **don't change anything
while the filter runs**. Each one outputs an *operation*, and dq collects
the operations into a *plan*. After the filter finishes:

1. dq prints any ordinary values as they are output, as usual.
2. dq checks the whole plan against itself and the current filesystem.
   See [Plan checks](#plan-checks).
3. Without `--apply`, dq prints the plan and stops. This is a dry run.
4. With `--apply`, dq runs the operations in the order the filter output them.

```sh
$ dq 'files | select(.ext == "jpeg") | mv(.stem + ".jpg")'
mv     photos/a.jpeg -> photos/a.jpg
mv     photos/b.jpeg -> photos/b.jpg
dq: dry run: 2 operations; add --apply to run

$ dq --apply 'files | select(.ext == "jpeg") | mv(.stem + ".jpg")'
```

**Always run without `--apply` first**, and read the plan.

Each action takes an entry or a path string as its input.

### `rm`

Removes the input. A directory is removed together with everything in it.
A symlink is removed on its own; its target is left alone.

```sh
dq 'files | select(.name | endswith(".orig")) | rm'
dq 'dirs | select(.name == "__pycache__") | rm'
```

### `mv($to)`

Moves or renames the input to `$to`. **`$to` is relative to the directory
that contains the input**, not to the working directory. This way a rename
needs only the new name:

```sh
dq 'at("src/old.rs") | mv("new.rs")'        # src/old.rs -> src/new.rs
dq 'at("src/old.rs") | mv("../old.rs")'     # src/old.rs -> old.rs
dq 'files | select(.ext == "JPG") | mv(.stem + ".jpg")'
```

- An absolute `$to` is used as it is.
- `$to` must not already exist.
- dq renames the entry in place, so it can't move an entry to a different
  filesystem yet.

### `cp($to)`

Copies the input to `$to`. `$to` is resolved the same way as for `mv`.
Directories are copied with everything in them. Symlinks are copied as
symlinks.

```sh
dq 'at("config.toml") | cp("config.toml.bak")'
```

`$to` must not already exist.

### `write($content)`

Writes the string `$content` to the input path. If the file doesn't exist,
dq creates it; if it does, dq replaces its contents.

```sh
dq 'child("VERSION") | write("0.2.0\n")'
dq 'files | select(.ext == "md") | write(content | gsub("\\bteh\\b"; "the"))'
```

- `$content` must be a string. Use `tojson` to write other values.
- The parent directory must exist, or be created by a `mkdir` in the same plan.
- `write` can't replace a directory.

### `mkdir`

Creates the input directory, and any missing parent directories. If the
directory already exists, `mkdir` does nothing.

```sh
dq '(child("out") | mkdir), (child("out/index.txt") | write("hello\n"))'
```

### Plan checks

dq checks the whole plan before it runs any operation. If it finds a problem,
it lists all of them, changes nothing, and exits with status 1.

| Problem | Example |
| --- | --- |
| The source of `rm`, `mv` or `cp` doesn't exist | `"missing" \| rm` |
| The target of `mv` or `cp` already exists | `at("a") \| mv("b")` when `b` exists |
| `write` would replace a directory | `at("src") \| write("x")` |
| The target's parent directory doesn't exist and no `mkdir` creates it | `child("no/such/dir/f") \| write("x")` |
| Two operations create the same path | two files moved to the same name |
| An operation removes a path (`rm`, or the source of `mv`) that another operation uses | `(at("src") \| rm), (at("src/a") \| mv("b"))` |

If two operations are exactly the same, dq keeps one of them. So when a
filter reaches the same entry twice, the result is still correct.

The checks happen before anything runs, but the filesystem can still change
between the check and the run. If an operation fails during `--apply`, dq
stops, reports the error, and tells you how many operations had already run.

### How operations are represented

An action outputs an object with the key `"dq:op"`:

```sh
$ dq -c 'at("a.txt") | rm | tojson'
"{\"dq:op\":\"rm\",\"path\":\"a.txt\"}"
```

dq treats **any** output object that has a `"dq:op"` key as an operation.
Don't use that key in your own objects.

## Paths

There are two sorts of paths, and they are resolved differently:

- **Entry paths.** The `path` field of each entry starts with `PATH` as you
  gave it on the command line. `at`, `ls`, `tree` and `child` build on it.
  If `PATH` is `.`, dq leaves out the `./` prefix, so paths look like
  `src/main.rs`.
- **Plain strings.** A string you write yourself, such as `"notes" | mkdir`,
  is resolved against the **working directory**, not against `PATH`.

The two are the same when `PATH` is `.`, which is the default. They are
different when you give another `PATH`:

```sh
dq '"out" | mkdir' project             # creates ./out
dq 'child("out") | mkdir' project      # creates ./project/out
```

To make sure a path is inside the tree you are querying, build it from an
entry with `child` or `at`.

dq doesn't normalize entry paths, so `at("src") | at("..")` has the path
`src/..`. Operation paths are normalized, so `mv("../x")` from `src/a` becomes `x`.

## Command line

| Option | Description |
| --- | --- |
| `<FILTER>` | The jq filter to run. |
| `[PATH]` | The entry to start from. The default is `.`. It can be a file. |
| `--apply` | Run the plan instead of printing it. |
| `-r`, `--raw-output` | Print strings without quotes. |
| `-c`, `--compact-output` | Print each value on one line. |
| `-M`, `--monochrome-output` | Don't color the output. dq also turns off color when stdout is not a terminal, or when `NO_COLOR` is set. |

### Exit status

| Status | Meaning |
| --- | --- |
| 0 | Success. |
| 1 | The plan failed its checks, or an operation failed during `--apply`. |
| 2 | Bad arguments, or `PATH` doesn't exist. |
| 3 | The filter doesn't parse or uses an undefined name. |
| 5 | The filter raised an error while it ran. |

## Pitfalls

**`|` binds more loosely than `,`.** `at("src") | rm, (at("x") | rm)` means
`at("src") | (rm, (at("x") | rm))`, so the second `at` is relative to `src`.
Put parentheses around each part:

```sh
dq '(at("src") | rm), (at("x") | rm)'
```

**`..` is jq's `..`, not a directory walk.** It goes through the values
inside the current JSON value, which for an entry means its fields. Use
`tree` to walk directories.

**`tree` leaves out its input.** `dq 'tree'` lists everything below `.`, but not `.` itself.

**`files` goes into every directory, including `.git`, `node_modules` and
`target`.** On large trees, leave them out with `tree(f)`.

**Directory sizes are not the sizes of their contents.** For the total size
under a directory, add up its files: `[files | .size] | add`.
