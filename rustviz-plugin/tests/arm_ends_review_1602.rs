// Shapes from the independent review of #1602 (borrow ends placed in
// branch arms), each drawn true: a one-line conditional's reference used
// again after it on the line keeps its end there; the end of a loan an
// arm made stays in that arm; a reference moved on in one nested arm ends
// its borrow through what it was moved into; a reference used again in
// the next iteration of a loop doesn't end in a nested arm; two
// references copied into each other on one line render (no endless
// reordering); a lender drawn in an outer conditional is given back on
// the `None` path of an `if let` nested in it; a reference whose borrow
// ended in each arm, or an unused or-pattern binding, isn't said to have
// "no value" at a later join.
fn used_after_on_line(c: bool) {
    let s1 = String::from("a");
    let r1 = &s1;
    let n = (if c { r1.len() } else { 0 }, r1.len());
}

fn loan_made_in_arm(c: bool) {
    let a2 = String::from("a");
    let mut b2 = String::from("b");
    let mut r2: &String = &a2;
    if c { r2.len(); } else { r2 = &mut b2; }
}

fn moved_in_nested_arm(r3: &mut String, q3: &String, c: bool, d: bool) -> usize {
    if c {
        if d {
            let moved = r3;
            moved.push('x');
        } else {
            r3.len();
        }
    }
    q3.len()
}

fn in_loop(c: bool, d: bool) {
    let s4 = String::from("a");
    let q4 = &s4;
    let r4 = &s4;
    for _i in 0..4 {
        if c { } else if d { } else { let k = r4.len(); }
    }
    println!("{}", q4);
}

fn copied_both_ways(c: bool) {
    let s5 = String::from("a");
    let t5 = String::from("b");
    let mut a5 = &s5;
    let mut b5 = a5;
    if c {
        b5 = a5; a5 = b5; a5.len(); b5.len();
    } else {
        println!("{}", t5);
    }
    println!("{}", t5);
}

fn outer_column(v6: Vec<String>, c: bool, e: bool, s6: String) {
    if e {
        if let Some(x6) = v6.first() {
            if c {
                drop(s6);
                let o = x6.clone();
            }
        }
    } else {
        v6.len();
    }
}

fn ended_in_each_arm(b: bool, d: bool, e: bool) {
    let mut s7 = String::from("a");
    if b {
        let r7 = &mut s7;
        if d {
            r7.push('x');
        } else {
            r7.len();
        }
        if e { return; }
    }
}

fn or_pattern(t8: (String, String, bool), c: bool) {
    match t8 {
        (ref a8, _, true) | (_, ref a8, false) => {
            if c { return; }
        }
    }
}
