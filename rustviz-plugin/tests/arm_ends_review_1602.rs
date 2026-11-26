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

// Round 2 of the review: one-line conditionals whose reference's loan
// outlives the arm keep main's join-row end.
fn returned_call_result<'a>(a9: &'a mut Vec<i32>, b9: &'a mut Vec<i32>) -> Option<&'a mut i32> {
    if a9.is_empty() { b9.first_mut() } else { a9.first_mut() }
}

fn two_phase_receiver(c: bool) {
    let mut v10 = vec![1];
    let r10 = &mut v10;
    r10.push(if c { r10.len() } else { 0 });
}

struct H11<'a> { p: &'a String }

fn copied_out_of_arm(c: bool) {
    let a11 = String::from("a");
    let r11 = &a11;
    let h11 = if c { H11 { p: r11 } } else { H11 { p: r11 } }; h11.p.len();
}

fn id<T>(x: T) -> T { x }

fn moved_through_call(r12: &mut String, q12: &String, c: bool, d: bool) -> usize {
    if c {
        if d {
            let moved = id(r12);
            moved.push('x');
        } else {
            r12.len();
        }
    } else {
        r12.len();
    }
    q12.len()
}

// Round 5 of the review: a nested `if` whose arm calls an `Fn` closure
// (which records no event), reborrows in place, or calls an unrelated
// `FnOnce` closure before `return` still uses what it may use: the loan
// isn't ended as the nested `if` starts (an earlier head of #1602 did,
// and the `FnOnce` case failed to render).
fn fn_closure_in_nested_return(a: bool, c: bool) {
    let s13 = String::from("hi");
    let f13 = || s13.len();
    if a {
        if c {
            f13();
            return;
        }
    } else {
        f13();
    }
    drop(s13);
}

fn reslice_in_nested_return(a: bool, c: bool) {
    let mut s14 = String::from("hi");
    let mut r14 = &mut s14;
    if a {
        if c {
            r14 = &mut *r14;
            return;
        }
    } else {
        r14.len();
    }
    s14.push('b');
}

fn unrelated_fnonce_in_nested_return(a: bool, c: bool) {
    let s15 = String::from("hi");
    let t15 = String::from("t");
    let r15 = &s15;
    let f15 = || drop(t15);
    if a {
        if c {
            f15();
            return;
        }
    } else {
        r15.len();
    }
    drop(s15);
}

// Round 6: the nested `if`'s condition reads the reference (`*r16`, no
// event), so its borrow doesn't end before it; and an `if let Some(x) =
// v.first()` whose x is last used in a nested `if` keeps main's drawing
// (no v column there) when another reference still borrows v, or when
// the `else` declares another `x` (an earlier head gave v back on the
// `Some` path while other17 held it, and drew v18 "being shared"
// through the `else`).
fn condition_reads_ref(a: bool, d: bool, owned16: String) {
    let mut s16 = true;
    if a {
        let r16 = &mut s16;
        if d {
            drop(owned16);
            if *r16 { return; }
        } else {
            *r16 = false;
        }
    }
    s16 = false;
}

fn other_borrower_of_lender(v17: Vec<String>, c: bool, s17: String) {
    let other17 = &v17[0];
    if let Some(x17) = v17.first() {
        if c {
            drop(s17);
            let o = x17.clone();
        }
    }
    other17.len();
}

fn same_name_in_else(v18: Vec<String>, c: bool, s18: String, t18: String) {
    if let Some(x18) = v18.first() {
        if c {
            drop(s18);
            let o = x18.clone();
        }
    } else {
        let x18 = &t18;
        let y = x18.len();
    }
}

// Round 7: r20 re-pointed (`r20 = &mut b20;`) before it is moved on
// (`let next20 = r20;`): its loan of a20 ends at the re-pointing, so
// `drop(a20)` is fine (an earlier head kept a20 lent down to it). An
// `if let` on the same line as the outer `if`'s split: v21 is given back
// on the `if let`'s None path, not in the outer `else`, where
// `v21.first()` never ran. A closure capturing v22 holds its loan through
// its `Fn` call: v22 isn't given back on the None path.
fn repointed_then_moved(c: bool) {
    let mut a20 = String::from("a");
    let mut b20 = String::from("b");
    let mut r20 = &mut a20;
    if c {
        r20 = &mut b20;
        let next20 = r20;
        drop(a20);
        next20.push('x');
    } else {
        r20.len();
    }
}

fn if_let_on_split_line(v21: Vec<String>, c: bool, e: bool, s21: String) {
    if e { if let Some(x21) = v21.first() {
            if c {
                drop(s21);
                let o = x21.clone();
            }
        }
    } else {
        v21.len();
    }
}

fn closure_holds_lender(v22: Vec<String>, c: bool, e: bool, s22: String) {
    let other22 = || v22.len();
    if e {
        if let Some(x22) = v22.first() {
            if c {
                drop(s22);
                let o = x22.clone();
            }
        }
    } else {
        v22.len();
    }
    other22();
}
