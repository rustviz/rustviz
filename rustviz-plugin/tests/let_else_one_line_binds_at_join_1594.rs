// #1594: a one-line let-else drawn as a branch (s is dropped at the
// `return`) forks a row above its line and joins a row below it. The
// pattern binds only on the path that skipped the `else` block, so its
// bindings (and the scrutinee's dot for them) are drawn at the join, not
// on the line's own row between the fork and the join, where x's column
// started on the `else` path's sub-row, above the join.
fn copy_out(o: Option<i32>) {
    let s = String::new();
    let Some(x) = o else { return };
    println!("{}{}", x, s);
}

fn by_move(o: Option<String>) {
    let s = String::new();
    let Some(x) = o else { return };
    println!("{}{}", x, s);
}

fn by_call(v: Vec<String>) {
    let s = String::new();
    let Some(x) = v.first() else { return };
    println!("{}{}", x, s);
}

fn holder(o: Option<String>) {
    let s = String::new();
    let Some(x) = &o else { return };
    println!("{}{}", x, s);
}

fn take_call(mut o: Option<String>) {
    let s = String::new();
    let Some(x) = o.take() else { return };
    println!("{}{}{:?}", x, s, o);
}

fn tuple(p: Option<(String, i32)>) {
    let s = String::new();
    let Some((a, n)) = p else { return };
    println!("{}{}{}", a, n, s);
}

fn in_arm(o: Option<i32>, c: bool) {
    let s = String::new();
    if c {
        let Some(x) = o else { return };
        println!("{}{}", x, s);
    }
}

fn then_more(o: Option<i32>) {
    let s = String::new();
    let t = String::new();
    let Some(x) = o else { return }; let y = x; drop(t);
    println!("{}{}", y, s);
}

// In a `match` arm (the let-else isn't the arm's own conditional).
fn in_match_arm(o: Option<i32>, k: u8) {
    let s = String::new();
    match k {
        0 => {
            let Some(x) = o else { return };
            println!("{}{}", x, s);
        }
        _ => {}
    }
}

// A local bound on the line before the let-else and copied out of after
// it: it has dots on both sides of the let-else, so all its dots stay on the
// line's row, as before #1594 (y's copy from it still binds at the join;
// the arrow between them slants: #1767).
fn local_before(o: Option<i32>) {
    let s = String::new();
    let k = 12; let Some(x) = o else { return }; let y = k;
    println!("{}{}{}", x, y, s);
}

// A block's local dropped as the block closes, before the let-else on
// the same line: its drop stays on the line's row.
fn block_before(o: Option<i32>) {
    let s = String::new();
    { let t = String::new(); } let Some(x) = o else { return };
    println!("{}{}", x, s);
}

// The binding then assigned to a `let n;` local in an arm: n's line
// (no value yet) runs down to its dot at the join.
fn assigned_in_arm(o: Option<i32>, c: bool) {
    let s = String::new();
    let n;
    if c {
        let Some(x) = o else { return }; n = x;
    } else {
        n = 1;
    }
    println!("{}{}", n, s);
}

// A named local whose whole life is on the line (a block): its dots stay
// on the row, as for `local_before` (#1767).
fn in_block(o: Option<i32>) {
    let s = String::new();
    { let k2 = 12; let Some(x) = o else { return }; let y = k2; }
    println!("{}", s);
}

fn main() {}
