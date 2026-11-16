// `v = if c { f } else { g };` where v already owns a String (#1010): only
// the branch that ran moves its String into v, and v's old String is
// dropped there, where the new one arrives (once the branch's value is
// computed). The other branch's String stays where it was and is dropped
// at the end of the function (Rust tracks which with a runtime drop flag).
fn reassign(c: bool, f: String, g: String) -> String {
    let mut v = String::from("old");
    v = if c { f } else { g };
    v
}

// The same with a `match`, one arm per line.
fn reassign_match(n: u8, p: String, q: String) -> String {
    let mut w = String::from("old");
    w = match n {
        0 => p,
        1 => q,
        _ => String::new(),
    };
    w
}

// A branch that moves u's old String away first: nothing is left to drop
// when the new one arrives.
fn moved_first(c: bool, a: String, b: String) -> String {
    let mut u = String::from("old");
    u = if c {
        drop(u);
        a
    } else {
        b
    };
    u
}

// A branch that returns gives x no new value: x's old String is dropped at
// the `return`, with h.
fn returns(c: bool, h: String, k: String) -> String {
    let mut x = String::from("old");
    x = if c {
        h
    } else {
        return k;
    };
    x
}

// A branch whose value is y's own String hands it back to y: nothing is
// dropped on that path.
fn keeps(c: bool, m: String) -> String {
    let mut y = String::from("old");
    y = if c { y } else { m };
    y
}
