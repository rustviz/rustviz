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

fn main() {
    through_call();
    plain_mut();
    unused_before();
}
