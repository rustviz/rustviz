// A reference whose last use is in one arm of an `if` / `match` doesn't
// outlive a borrow made and ended in another arm: on that arm's path its
// borrow ended as the arm started. The arm's own reference was the
// lender's last borrower there and gives it back where it ends; it was
// drawn ending with nothing given back, the lender still borrowed through
// the join and `State::Invalid` after it (#1425). The same for a
// reference declared in an `else if` arm and last used in the `else` of
// a conditional nested in it (#1428).
fn mut_in_then_shared_in_else() {
    let mut s1 = String::from("x");
    let x1 = &s1[..];
    let c = true;
    if c {
        let m1 = &mut s1;
    } else {
        println!("{} {}", s1, x1);
    }
    let n1 = s1.len();
}

fn mut_in_match_arm() {
    let mut s2 = String::from("x");
    let x2 = &s2;
    let o = Some(1);
    match o {
        Some(_) => {
            let m2 = &mut s2;
            m2.push('a');
        }
        None => {
            println!("{}", x2);
        }
    }
    let n2 = s2.len();
}

fn mut_in_nested_if() {
    let mut s3 = String::from("x");
    let x3 = &s3;
    let c = true;
    let d = false;
    if c {
        if d {
            let m3 = &mut s3;
            m3.push('a');
        }
    } else {
        println!("{}", x3);
    }
    let n3 = s3.len();
}

fn shared_in_then_then_moved() {
    let s4 = String::from("x");
    let x4 = &s4;
    let c = true;
    if c {
        let m4 = &s4;
        println!("{}", m4);
    } else {
        println!("{}", x4);
    }
    let k4 = s4;
}

fn mut_in_if_let_then_moved() {
    let mut s5 = String::from("x");
    let x5 = &s5;
    let o = Some(2);
    if let Some(_v) = o {
        let m5 = &mut s5;
        m5.push('b');
    } else {
        println!("{}", x5);
    }
    let k5 = s5;
}

fn take(m: &mut String) { m.push('!'); }

fn mut_moved_into_call() {
    let mut s6 = String::from("x");
    let x6 = &s6;
    let c = true;
    if c {
        let m6 = &mut s6;
        take(m6);
    } else {
        println!("{}", x6);
    }
    let k6 = s6;
}

// (The `else if` functions are in this order because a dead-loan note
// lands on the wrong row of the second one when the o7 one comes first:
// #1443.)
fn else_if_shared_then_mut_in_nested_if() {
    let mut s8 = String::from("a");
    let n = 1;
    if n > 2 {
        println!("{}", s8);
    } else if n > 0 {
        let r8 = &s8;
        if n > 1 {
            let m8 = &mut s8;
        } else {
            println!("{}", r8);
        }
        println!("{}", s8);
    }
}

fn else_if_ref_used_in_nested_else() {
    let mut o7 = Some(String::from("a"));
    let n = 1;
    if n > 2 {
        println!("{:?}", o7);
    } else if n > 0 {
        let r7 = o7.as_ref().unwrap();
        if n > 0 {
            let m7 = o7.as_mut().unwrap();
        } else {
            let w7 = r7;
            o7.take();
        }
        println!("{:?}", o7);
    }
}
