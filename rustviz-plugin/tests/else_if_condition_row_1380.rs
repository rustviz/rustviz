// #1380: `if a {}`'s borrow end and `look(r)`'s arrows share the
// `} else if look(r) {` row, so the `else if` arm's events go on a row
// of their own below it. The arm still starts on its condition's row:
// the leg beside it is that arm's, not dashed as "not part of that
// branch".
fn look(x: &i32) -> bool { *x > 0 }
fn repro(a: bool) {
    let e = 0;
    let r = &e;
    if a {
    } else if look(r) {
    } else {
    }
    println!("{}", e);
}
fn else_if_let(a: bool) {
    let e = 0;
    let r = &e;
    if a {
    } else if let Some(v) = Some(look(r)) {
        println!("{}", v);
    }
    println!("{}", e);
}
fn three_level(a: bool, b: bool) {
    let e = 0;
    let r = &e;
    if a {
    } else if b {
    } else if look(r) {
    } else {
    }
    println!("{}", e);
}
fn in_arm(a: bool, c: bool) {
    let e = 0;
    let r = &e;
    if c {
        if a {
        } else if look(r) {
        } else {
        }
    }
    println!("{}", e);
}
fn main() {}
