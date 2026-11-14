// #796 near misses: writes to a lender that compile without the "On this
// path ... is already over here" note, because the lender isn't drawn
// borrowed at the write. (A write on a path where the loan is live, e.g.
// `if y > 5 { x = 2; } else { c = &y; }` with `c` read after the `if`,
// is rejected by rustc, E0506, so it can't be drawn at all.)

// The arm that writes doesn't read c, the other does: x's column is
// drawn per arm, and in the writing arm the borrow has already ended.
fn split(y: i32) {
    let mut x = 8;
    let c = &x;
    if y > 5 {
        x = 2;
    } else {
        println!("{}", c);
    }
    println!("{}", x);
}

// The write comes after the reference's last use on every path.
fn after_last_use() {
    let mut a = 8;
    let b = 9;
    let mut r = &a;
    if b > 5 {
        r = &b;
    }
    println!("{}", r);
    a = 2;
    println!("{}", a);
}

fn main() {
    split(1);
    after_last_use();
}
