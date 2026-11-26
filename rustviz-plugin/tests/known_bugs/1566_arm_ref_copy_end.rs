// Known bug, from the closed 2.x fix PRs (fix/r2-arms-borrow-ends); kept as a RustViz 3 regression case.
// Issue(s): #1566 (see known_bugs/README.md for what main draws wrong).
// #1566: a copy of a shared reference made in an arm and never used
// (`let h = r;`) ends its borrow on its own `let` line, in its arm. r
// still borrows the lender, so h's end gives nothing back, and it was
// left outside the arm: drawn in a later arm, after "h goes out of
// scope".
struct Holder<'a> { r: &'a String }

fn slice_arm(v: Vec<String>, s: String) {
    let r = &s;
    match &v[..] {
        [] => {}
        [x2] => {
            let h = r;
        }
        [x, rest @ ..] => {}
    }
    println!("{}", r);
}

fn other_arm_reborrows(s2: String, c: bool, m: &mut String, n: i32) -> usize {
    let r2 = &s2;
    if c {
        let r3 = r2;
    } else {
        match n {
            1 => { let m3 = &mut *m; }
            _ => {}
        }
    }
    r2.len()
}

fn holder_in_arm(v: Vec<String>, s3: String) {
    let r4 = &s3;
    match &v[..] {
        [] => {}
        [y2] => {
            let hh = Holder { r: r4 };
        }
        [y, more @ ..] => {}
    }
    println!("{}", r4);
}
