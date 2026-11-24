// #1400: two `if let Some(e) = holder` with the same name `e` (each
// holder from `v.last_mut()`). The two `e`s share one column; the second
// one's scope end was taken from the first one's, so it gave v's
// borrow back nowhere: v stayed "mutably borrowed by e" and its state
// machine hit `State::Invalid` at the `println!`.
fn if_let() {
    let mut v = vec![String::from("a")];
    let o = v.last_mut();
    if let Some(e) = o {
        e.push('x');
    }
    let p = v.last_mut();
    if let Some(e) = p {
        e.push('y');
    }
    println!("{:?}", v);
}

// `match`, a third one with an `else`, and a shared borrow.
fn match_and_else() {
    let mut v = vec![String::from("a")];
    let o = v.last_mut();
    match o {
        Some(e) => e.push('x'),
        None => {}
    }
    let p = v.last_mut();
    match p {
        Some(e) => e.push('y'),
        None => {}
    }
    let q = v.last_mut();
    if let Some(e) = q {
        e.push('z');
    } else {
        println!("none");
    }
    let w = v.first();
    if let Some(e) = w {
        println!("{}", e);
    }
    let x = v.first();
    if let Some(e) = x {
        println!("{}", e);
    }
    println!("{:?}", v);
}

// Different lenders, the first one in a block of its own, and a `for`
// loop's variable of the same name first.
fn others() {
    let mut v = vec![String::from("a")];
    let mut w = vec![String::from("b")];
    {
        let o = v.last_mut();
        if let Some(e) = o {
            e.push('x');
        }
    }
    let p = w.last_mut();
    if let Some(e) = p {
        e.push('y');
    }
    for e in v.iter_mut() {
        e.push('z');
    }
    let q = v.last_mut();
    if let Some(e) = q {
        e.push('!');
    }
    println!("{:?} {:?}", v, w);
}

// `while let` on two iterators, the same name in each.
fn while_let() {
    let mut v = vec![String::from("a")];
    let mut it = v.iter_mut();
    while let Some(e) = it.next() {
        e.push('x');
    }
    let mut it2 = v.iter_mut();
    while let Some(e) = it2.next() {
        e.push('y');
    }
    println!("{:?}", v);
}

fn main() {
    if_let();
    match_and_else();
    others();
    while_let();
}
