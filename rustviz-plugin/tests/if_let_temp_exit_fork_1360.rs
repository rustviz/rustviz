// An edition-2024 `if let` (or let chain) scrutinee temporary that an exit
// in the matched block drops (#1360): its column forks at the `if`, like a
// `match` scrutinee's. The exit's leg ends in its drop there; the matched
// block's other path drops it at the block's end, and the else path before
// the `else` block. It used to be one straight line with two drops on it.
fn make() -> Vec<String> {
    vec![String::from("a")]
}

// The issue's program: the `return` is in an `if` nested in the block.
fn nested(n: usize) -> usize {
    if let Some(s) = make().pop() {
        if n == 0 {
            return 0;
        }
        s.len()
    } else {
        2
    }
}

// The `return` directly in the matched block.
fn direct(n: usize) -> usize {
    if let Some(s) = make().pop() {
        return s.len() + n;
    } else {
        2
    }
}

// No `else`, exit directly in the block.
fn no_else_direct(n: usize) -> usize {
    if let Some(s) = make().pop() {
        return s.len() + n;
    }
    1
}

// No `else`, exit nested: the drop at the `return` used to be missing.
fn no_else_nested(n: usize) -> usize {
    if let Some(s) = make().pop() {
        if n == 0 {
            return 0;
        }
        println!("{}", s);
    }
    1
}

// Two levels down: the drop at the `return` used to be missing.
fn deeper(n: usize) -> usize {
    if let Some(s) = make().pop() {
        if n > 1 {
            if n == 3 {
                return 0;
            }
        }
        s.len()
    } else {
        2
    }
}

// A `break` out of an enclosing loop.
fn in_loop(n: usize) -> usize {
    loop {
        if let Some(s) = make().pop() {
            if n == 0 {
                break;
            }
            println!("{}", s);
        } else {
            return 2;
        }
    }
    1
}

// A `?` in the matched block: its drop if it returns early used to be
// missing.
fn try_in_block(n: Option<usize>) -> Option<usize> {
    if let Some(s) = make().pop() {
        let k = n?;
        Some(s.len() + k)
    } else {
        None
    }
}

// The exit in a `match` arm in the block.
fn match_in_block(n: usize) -> usize {
    if let Some(s) = make().pop() {
        match n {
            0 => return 0,
            _ => {}
        }
        s.len()
    } else {
        2
    }
}

// Every path through the block exits.
fn both_exit(n: usize) -> usize {
    if let Some(s) = make().pop() {
        if n == 0 {
            return 0;
        } else {
            return s.len();
        }
    } else {
        2
    }
}

// An `else if let`.
fn else_if_let(n: usize, c: bool) -> usize {
    if c {
        1
    } else if let Some(s) = make().pop() {
        if n == 0 {
            return 0;
        }
        s.len()
    } else {
        2
    }
}

fn main() {
    nested(1);
    direct(1);
    no_else_direct(1);
    no_else_nested(1);
    deeper(1);
    in_loop(1);
    try_in_block(Some(1));
    match_in_block(1);
    both_exit(1);
    else_if_let(1, false);
}
