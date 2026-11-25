// #1527: `let r2 = r;` in a `match` arm, r a reborrow through m, both
// last used there. r2's end is drawn on the line's row and r's (with m's
// own end) on a row below it, but the arm's events stayed in their
// visit order, r's ends first: m's states ran back up from the lower
// row, its column stopped at the lower dot and its dashed "borrow has
// ended" rest began a row further down, in mid-air. Now the rest starts
// at that dot, and between the two dots m is still shared with r: r2's
// end says so ("... is no longer borrowed by r2, but is still immutably
// borrowed by r").
fn mutable() {
    let c = 1;
    let mut s = String::from("hi");
    let m = &mut s;
    let r = &*m;
    match c {
        0 => {
            let _r2 = r;
        }
        _ => {
            println!("{}", r);
        }
    }
}
fn shared() {
    let c = 1;
    let s = String::from("hi");
    let m = &s;
    let r = &*m;
    match c {
        0 => {
            let _r2 = r;
        }
        _ => {
            println!("{}", r);
        }
    }
}
fn option_arm() {
    let o = Some(1);
    let mut s = String::from("hi");
    let m = &mut s;
    let r = &*m;
    match o {
        Some(_) => {
            let _r2 = r;
            println!("x");
        }
        None => {
            println!("{}", r);
        }
    }
}
fn id<T>(t: T) -> T {
    t
}
fn maybe(s: String) -> Option<String> {
    Some(s)
}
// The generator's program (seed 202, bucket 01): a guarded arm.
fn guarded() {
    let mut s2 = String::from("hey");
    let s3 = s2.clone();
    let o = maybe(id(s3));
    let m = &mut s2;
    let r = &*m;
    match o {
        Some(x) if x.len() > 1 => {
            let mut _r2 = r;
        }
        Some(_) => {
            let _s4 = format!("{}!", r);
        }
        None => {
        }
    }
}
fn main() {
    mutable();
    shared();
    option_arm();
    guarded();
}
