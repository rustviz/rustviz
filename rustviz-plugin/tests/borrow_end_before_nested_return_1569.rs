// #1569: a borrow last used in the other arm, with an early `return` in a
// nested `if` of this arm. The borrower's scope ends at that `return`,
// but that isn't a use: its loan ends where this arm starts, before
// `drop(s)` / `s2.len()`. It was ended at the `return` and the nested
// join, after the move ("s's resource is no longer immutably borrowed"
// on a moved column), with s2 read while still mutably borrowed and its
// drop at the `return` stated as "No resource is dropped".
fn holder(c: bool) {
    let s = String::from("abc");
    let vr = vec![&s];
    if c {
        drop(s);
        if c {
            return;
        }
    } else {
        println!("{:?}", vr);
    }
}

fn declared_in_arm() {
    let mut s2 = String::from("abc");
    let b = true;
    if b {
        let m = &mut s2;
        if b {
            if s2.len() > 1 {
                return;
            }
        } else {
            m.push('a');
        }
    }
}

fn match_arm(c: bool, n: i32) {
    let s3 = String::from("abc");
    let r = &s3;
    match n {
        0 => {
            drop(s3);
            if c {
                return;
            }
        }
        _ => {
            println!("{}", r);
        }
    }
}
