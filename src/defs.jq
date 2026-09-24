# Walking

# All entries below the input, depth first. The input itself is not included.
def tree: ls | ., tree;
# Like `tree`, but only yield and descend into entries for which `f` is true.
def tree(f): ls | select(f) | ., tree(f);
def files: tree | select(.type == "file");
def dirs:  tree | select(.type == "dir");
