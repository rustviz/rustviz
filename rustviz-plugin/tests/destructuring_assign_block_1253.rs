// #1253: a destructuring assignment of a labeled block's value takes
// apart whichever value ended the block, a `break`'s or its last
// expression, where the block ends, as `let (x, n) = 'blk: { … }` binds
// it (#1226): s moves into x only if `break 'blk (s, 1)` ran, t only if
// the block reached its end. Main moved t unconditionally and never s,
// and gave n no new value.
fn assign_block(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut x = String::new();
    let mut n = 0;
    (x, n) = 'blk: {
        if c {
            break 'blk (s, 1);
        }
        (t, 2)
    };
    println!("{} {}", x, n);
}
fn assign_nested(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut x = String::new();
    let mut n = 0;
    let mut m = 0;
    ((x, n), m) = 'blk: {
        if c {
            break 'blk ((s, 1), 2);
        }
        ((t, 3), 4)
    };
    println!("{} {} {}", x, n, m);
}
fn assign_wild(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut x = String::new();
    (x, _) = 'blk: {
        if c {
            break 'blk (s, 1);
        }
        (t, 2)
    };
    println!("{}", x);
}
fn assign_two_breaks(c: bool, d: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let u = String::from("c");
    let mut x = String::new();
    let mut n = 0;
    (x, n) = 'blk: {
        if c {
            break 'blk (s, 1);
        }
        if d {
            break 'blk (t, 2);
        }
        (u, 3)
    };
    println!("{} {}", x, n);
}
fn main() {}
