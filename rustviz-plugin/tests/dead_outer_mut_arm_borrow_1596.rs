// A borrow made and ended in an arm where an outer `&mut` of the same
// lender was last used earlier in that arm (and is still used in the other
// arm): the lender gets the borrow back where it ends (#1596).

fn downgrade(s: &mut String) -> &String {
    s
}

// The issue's program.
fn through_call() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = downgrade(&mut s2);
        let m2 = &mut s2;
    } else {
        m.push('a');
    }
}

// A plain `&mut`.
fn plain_mut() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = &mut s2;
        let m2 = &mut s2;
    } else {
        m.push('a');
    }
}

// Already right: m not used in the arm before r3's borrow.
fn unused_before() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let r3 = downgrade(&mut s2);
        let m2 = &mut s2;
    } else {
        m.push('a');
    }
}

// o is read after the `if`, so its loan is live on the `if` path too: r's
// end gives s nothing back.
fn outer_read_after_join() {
    let s = String::from("a");
    let o = &s;
    if o.len() > 0 {
        let n = o.len();
        let r = &s;
        println!("{}", r);
    } else {
        println!("{}", o);
    }
    println!("{}", o.len());
}

// m's last use on this path and r3's borrow on one line.
fn same_line() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len(); let r3 = &mut s2;
        let m2 = &mut s2;
    } else {
        m.push('a');
    }
}

fn main() {
    through_call();
    plain_mut();
    unused_before();
    outer_read_after_join();
    same_line();
}
