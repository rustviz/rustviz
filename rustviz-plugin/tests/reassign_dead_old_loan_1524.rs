// A reference re-pointed in one arm (`r = &t;`) whose old loan is already
// dead before the `if` / `match` (r is never read again, or last read in
// the condition): the old loan ends there on every path, before the fork,
// not inside the arm on a row above it (#1524). The legs of r meet at a join
// dot that says its borrow ended before the branches.

// An explicit empty `else`.
fn empty_else() {
    let s = String::from("x");
    let t = String::from("y");
    let mut r = &s;
    if t.len() > 1 {
        r = &t;
    } else {
    }
}

// The re-point in the `else` arm.
fn in_else() {
    let s2 = String::from("a");
    let t2 = String::from("b");
    let mut r2 = &s2;
    if t2.len() > 1 {
    } else {
        r2 = &t2;
    }
}

// r copies another reference (`let mut r3 = x3;`); x3 is read in the other
// arm, so t3 stays borrowed by x3 there.
fn copied(c: bool) {
    let s3 = String::from("x");
    let t3 = String::from("y");
    let x3 = &t3;
    let mut r3 = x3;
    if c {
        r3 = &s3;
    } else {
        println!("{}", x3);
    }
}

// The lender moved in the arm (the generator's shape).
fn lender_moved(o: String, flag: bool) {
    let s4 = String::from("x");
    let mut r4 = &o;
    if flag {
        let _s5 = o;
        r4 = &s4;
    }
}

// The old loan's last use is the condition.
fn read_in_condition(arr: Vec<String>) {
    let s6 = String::from("rust");
    let mut r6 = &s6;
    if r6.is_empty() {
        r6 = &s6;
    } else {
        drop(arr);
    }
}

// In a match arm, with a `&mut`.
fn mut_in_match() {
    let mut s7 = String::from("a");
    let mut t7 = String::from("b");
    let mut r7 = &mut s7;
    match t7.len() {
        0 => r7 = &mut t7,
        _ => {}
    }
    s7.push('x');
}

fn main() {
    empty_else();
    in_else();
    copied(true);
    lender_moved(String::new(), true);
    read_in_condition(vec![]);
    mut_in_match();
}
