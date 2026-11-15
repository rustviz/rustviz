// The `else` block of a lone `if let` whose pattern always matches is
// dead code (#987): rustc warns `irrefutable_let_patterns`, and the
// compiled program has no else block. Nothing in it ever happens: g is
// never moved, and its String is dropped when g goes out of scope at the
// end of the function, with no drop flag. (rustc's borrow checker still
// checks the else block as if it could run.)
fn take(_s: String) {}

fn var_else(f: String, g: String) {
    if let x = f {
        println!("{}", x);
    } else {
        drop(g);
    }
}

// The same with a literal tuple taken apart the way it was built (#981):
// q's String is dropped with the tuple at the end of the if branch.
fn tuple_else(p: String, q: String, r: String) {
    if let (x, _) = (p, q) {
        println!("{}", x);
    } else {
        take(r);
    }
}

// The else's assignment never happens: y gets s1's String, s2 keeps its
// own until the end of the function.
fn both_assign(a: String, s1: String, s2: String) {
    let y;
    if let x = a {
        drop(x);
        y = s1;
    } else {
        y = s2;
    }
    println!("{}", y);
}

// Nor does its reassignment: h keeps its String.
fn reassign(b: String, mut h: String) {
    if let x = b {
        drop(x);
    } else {
        h = String::from("new");
    }
    println!("{}", h);
}

// Control: `Some(x)` can fail on o, so the else runs when o is None and k
// is moved only then.
fn refutable(o: Option<String>, k: String) {
    if let Some(x) = o {
        drop(x);
    } else {
        take(k);
    }
}
