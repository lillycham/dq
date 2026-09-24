def _path: if type == "string" then . else .path end;

# Paths

# Path of `$name` inside the input, which need not exist yet: `child("out") | mkdir`.
def child($name): _path | if . == "." then $name else . + "/" + $name end;

# Walking

# All entries below the input, depth first. The input itself is not included.
def tree: ls | ., tree;
# Like `tree`, but only yield and descend into entries for which `f` is true.
def tree(f): ls | select(f) | ., tree(f);
def files: tree | select(.type == "file");
def dirs:  tree | select(.type == "dir");

# Actions
#
# These do not touch the filesystem. They yield operations that dq collects
# into a plan, which it prints, or runs when given `--apply`.
# Each accepts an entry or a plain path string as input.
# Plain strings are relative to the working directory; use `child` to build paths from entries.

def _op($op): {"dq:op": $op, "path": _path};

def rm: _op("rm");
# `$to` is relative to the directory that contains the input.
def mv($to): _op("mv") + {"to": $to};
def cp($to): _op("cp") + {"to": $to};
def write($content): _op("write") + {"content": $content};
def mkdir: _op("mkdir");
