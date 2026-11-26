// Known bug, from the closed 2.x fix PRs (cx/hedge-f2); kept as a RustViz 3 regression case.
// Issue(s): #1638 (see known_bugs/README.md for what main draws wrong).
#![allow(irrefutable_let_patterns)]
fn main() {
    let mut n = 0;
    let mut r = &mut n;
    match &mut r {
        s => **s += 1,
    }
    *r += 1;
    match &r {
        shared => { let _value = **shared; }
    }
    if let arm = &mut r {
        **arm += 1;
    }
    while let round = &mut r {
        **round += 1;
        break;
    }
    let last = &mut r else { return };
    **last += 1;
    *r += 1;
}

struct Holder<'a> { r: &'a mut i32 }
fn borrow_field() {
    let mut n = 0;
    let mut h = Holder { r: &mut n };
    match &mut h.r {
        field => **field += 1,
    }
    *h.r += 1;
}
