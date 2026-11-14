// A constructor that a `match` / `if let` examines is never an actor
// (#807, decided in #873): `Some(f)` builds a temporary `Option` holding
// f's String, and a pattern that binds the payload by value (`Some(x)`)
// moves that String on to `x`. Drawn like a tuple-literal scrutinee
// (#327): f's String goes to `x`; in an arm that doesn't take it, f's
// String is dropped with the temporary when the match ends. A payload
// a call makes keeps that call's arrow; a macro's has none.
struct Wrapper(String);

fn make() -> String {
    String::from("r")
}

fn bind_drop(f: String) {
    match Some(f) {
        Some(x) => drop(x),
        None => {}
    }
}

fn if_let_bind(h: String) {
    if let Some(y) = Some(h) {
        println!("{}", y);
    }
}

fn if_let_macro() {
    if let Some(m) = Some(format!("m")) {
        println!("{}", m);
    }
}

fn match_call() {
    match Some(String::from("c")) {
        Some(c) => println!("{}", c),
        None => {}
    }
}

fn guard(g: String) {
    match Some(g) {
        Some(k) if k.len() > 3 => drop(k),
        _ => {}
    }
}

fn guard_call() {
    match Some(make()) {
        Some(r) if r.len() > 3 => println!("{}", r),
        _ => {}
    }
}

fn wrapper(w: String) {
    match Wrapper(w) {
        Wrapper(v) => println!("{}", v),
    }
}

fn result(a: String) {
    match Ok::<String, String>(a) {
        Ok(p) => drop(p),
        Err(q) => println!("{}", q),
    }
}

fn nested(b: String, n: i32) {
    match (Some(b), n) {
        (Some(t), 0) => drop(t),
        _ => {}
    }
}

fn main() {
    bind_drop(String::from("f"));
    if_let_bind(String::from("h"));
    if_let_macro();
    match_call();
    guard(String::from("g"));
    guard_call();
    wrapper(String::from("w"));
    result(String::from("a"));
    nested(String::from("b"), 0);
}
