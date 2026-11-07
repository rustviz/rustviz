// #605, #623: more temporaries that something is moved out of. What is
// left in them is dropped with them (edition 2024 timing): a `let`-`else`
// scrutinee right after its pattern is matched (before the `else` block
// runs), a struct update's base at the end of its statement, a temporary
// whose field is matched on when the match ends, a plain `let`'s at the
// end of the statement.
struct P {
    name: String,
    tag: String,
}

struct R {
    n: i32,
    tag: String,
}

struct K {
    name: String,
    n: i32,
}

struct O {
    opt: Option<String>,
    tag: String,
}

struct W {
    pair: (String, String),
    other: String,
}

fn make_p() -> P {
    P { name: String::from("a"), tag: String::from("t") }
}

fn make_r() -> R {
    R { n: 1, tag: String::from("t") }
}

fn make_k() -> K {
    K { name: String::from("k"), n: 1 }
}

fn make_o() -> O {
    O { opt: Some(String::from("o")), tag: String::from("t") }
}

fn make_w() -> W {
    W { pair: (String::from("a"), String::from("b")), other: String::from("o") }
}

fn res() -> Result<String, String> {
    Ok(String::from("r"))
}

fn pair() -> (String, String) {
    (String::from("a"), String::from("b"))
}

fn opt_pair() -> Option<(String, String)> {
    Some((String::from("a"), String::from("b")))
}

fn main() {
    let Ok(s) = res() else { return; };
    let Some((b, _)) = opt_pair() else {
        return;
    };
    let Some(x) = make_o().opt else { return; };
    let q = P { name: String::from("x"), ..make_p() };
    // Only an i32 is left in make_r()'s temporary: no column.
    let r = R { n: 2, ..make_r() };
    // The base's fields that K takes are Copy: the base is left whole.
    let k = K { name: String::from("k"), ..make_k() };
    match make_p().name {
        n => println!("{}", n),
    }
    if let Some(y) = make_o().opt {
        println!("{}", y);
    }
    // A plain `let` pattern that takes the value (or a field) apart
    // leaves the `_` parts in the temporary (#623).
    let (c, _) = make_w().pair;
    let (d, _) = pair();
    println!("{} {} {} {} {} {} {} {} {}", s, b, x, q.tag, r.n, k.name, k.n, c, d);
}
