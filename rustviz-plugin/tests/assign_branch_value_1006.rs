// `v = if c { f } else { g };` assigns a `let v;` local declared earlier:
// only the branch that ran moved its String into v (#1006). The other one
// keeps its String, which is dropped when it goes out of scope (Rust tracks
// which with a runtime drop flag), as for `let v = if c { f } else { g };`.
fn assign(c: bool, f: String, g: String) -> String {
    let v;
    v = if c { f } else { g };
    v
}

// The same with a `match`.
fn assign_match(n: i32, p: String, q: String) -> String {
    let w;
    w = match n {
        0 => p,
        _ => q,
    };
    w
}

// A branch whose value is its own variable moves that variable's String
// into u (#994).
fn assign_local(c: bool, a: String, b: String) -> String {
    let u;
    u = if c {
        let x = a;
        x
    } else {
        b
    };
    u
}
