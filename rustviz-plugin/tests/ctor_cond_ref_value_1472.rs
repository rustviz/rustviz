// #1472: `Some(if c { let r = &s1; r } else { .. })` gives y a value
// that holds a copy of r's reference (for a `&mut`, r's reference moved
// into it), not a copy of r: y isn't a reference. Drawn as `let y =
// Some(r);` and `let y = [r];` are: a borrow from what r borrows into y.
// Also with r declared before the conditional, a `match`, a `&mut`, and
// an assignment whose branches declare references with the same name
// (each branch's value is paired with the holder in that branch, while
// `r` is still that branch's; also for `q = if c { let r = &a; Some(r) }
// else { let r = &b; Some(r) }`, #1463).
fn branch_locals() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let y = Some(if n > 2 {
        let r = &s1;
        r
    } else {
        let r = &s2;
        r
    });
    println!("{:?}", y);
}

fn declared_before() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let r0 = &s1;
    let y0 = Some(if n > 2 { r0 } else { &s2 });
    println!("{:?} {}", y0, r0);
}

fn match_arms() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let ym = Some(match n {
        2 => {
            let rm = &s1;
            rm
        }
        _ => {
            let rm = &s2;
            rm
        }
    });
    println!("{:?}", ym);
}

fn mutable() {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let n = s1.len();
    let yu = Some(if n > 2 {
        let ru = &mut s1;
        ru
    } else {
        let ru = &mut s2;
        ru
    });
    println!("{:?}", yu);
    println!("{} {}", s1, s2);
}

fn assigned() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let mut ya = None;
    println!("{:?}", ya);
    ya = Some(if n > 2 {
        let ra = &s1;
        ra
    } else {
        let ra = &s2;
        ra
    });
    println!("{:?}", ya);
}

fn assigned_some() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let mut q = None;
    println!("{:?}", q);
    q = if n > 2 {
        let rq = &s1;
        Some(rq)
    } else {
        let rq = &s2;
        Some(rq)
    };
    println!("{:?}", q);
}

fn main() {
    branch_locals();
    declared_before();
    match_arms();
    mutable();
    assigned();
    assigned_some();
}
