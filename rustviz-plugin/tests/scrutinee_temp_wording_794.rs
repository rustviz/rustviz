// The temporary a `match` / `if let` examines (`Some(c)`) is dropped when
// the `match` ends / when the `if let` is done, not "at the end of this
// statement" (a `match` can be a block's final expression). When a
// pattern binds the String by value (`Some(x)`), it moves on to `x`
// instead; the dot on `c` must not say it is dropped with the temporary.
// Checked with a Drop-printing program: `Some(_)` drops after the arm,
// `Some(x)` drops at `drop(x)` / the arm's end.
fn wild(c: String) {
    match Some(c) {
        Some(_) => {}
        None => {}
    }
}

fn wild_stmt(d: String) -> usize {
    match Some(d) {
        Some(_) => {}
        None => {}
    };
    1
}

fn if_let_wild(e: String) {
    if let Some(_) = Some(e) {
        println!("some");
    }
}

fn bind_drop(f: String) {
    match Some(f) {
        Some(x) => drop(x),
        None => {}
    }
}

fn bind_print(g: String) {
    match Some(g) {
        Some(x) => println!("{}", x),
        None => {}
    }
}

fn if_let_bind(h: String) {
    if let Some(x) = Some(h) {
        println!("{}", x);
    }
}

fn main() {
    wild(String::from("c"));
    wild_stmt(String::from("d"));
    if_let_wild(String::from("e"));
    bind_drop(String::from("f"));
    bind_print(String::from("g"));
    if_let_bind(String::from("h"));
}
