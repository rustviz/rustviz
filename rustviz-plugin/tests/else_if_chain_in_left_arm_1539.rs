// #1539: an `else if` chain nested in the left arm of a conditional,
// whose last `else` holds an `if` that returns. The chain cascades right
// (then-arm, `else if`, `else`, the inner `if`'s diamond on s's column),
// further than the room `branch_centers` kept for the left arm: the
// inner diamond's fork, join and drop(s)'s dot were drawn on the outer
// `if`'s else leg, which ran through them. The outer else leg now sits
// right of everything the left arm reaches.
fn run(c: bool, d: bool) {
    let s = String::from("hi");
    if c {
        if d {
        } else if d {
        } else {
            if d { return; }
            drop(s);
        }
    }
}
fn in_match(n: u8, d: bool) {
    let s = String::from("hi");
    match n {
        0 => {
            if d {
            } else if d {
            } else {
                if d { return; }
                drop(s);
            }
        }
        1 => {}
        _ => {}
    }
}
fn in_if_let(o: Option<i32>, d: bool) {
    let s = String::from("hi");
    if let Some(_x) = o {
        if d {
        } else if d {
        } else {
            if d { return; }
            drop(s);
        }
    } else {
        println!("{}", s);
    }
}
fn maybe(s: String) -> Option<String> {
    Some(s)
}
// The generator's program (seed 505, bucket 07).
fn closure_in_last_else() {
    let s = String::from("hi");
    let o = maybe(String::from("hey"));
    if let Some(_x) = o.as_ref() {
        if o.is_some() {
        } else if o.is_some() {
        } else {
            if o.is_some() { return; }
            let _c2 = move || s;
        }
    }
}
fn main() {
    run(true, false);
    in_match(0, false);
    in_if_let(Some(1), false);
    closure_in_last_else();
}
