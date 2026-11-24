// #1386: a `match` / `if let` / `while let` scrutinee that isn't a plain
// variable is held in a temporary whose column forks into the arms, as a
// variable's does. An arm whose pattern binds by value takes its binding
// from the temporary ("Move from (temporary) to s"); where that leaves
// nothing that needs drop in it, the temporary's leg ends there. The other
// arms keep the value, and the temporary is dropped where Rust drops it:
// when the `match` ends, or (edition 2024) before an `if let`'s `else`
// block runs. Checked with rustc and a type that prints on drop.
struct P {
    a: Option<String>,
    b: String,
}

struct Q {
    name: Option<String>,
    tag: String,
}

fn make() -> String {
    String::from("m")
}

fn make_opt() -> Option<String> {
    Some(String::from("o"))
}

fn make_res() -> Result<String, String> {
    Ok(String::from("r"))
}

fn make_q() -> Q {
    Q { name: None, tag: String::new() }
}

// A guard can't move: the arm takes r only once it passes; otherwise the
// `_` arm runs and the temporary is dropped when the match ends.
fn guard_call() {
    match make_opt() {
        Some(r) if r.len() > 3 => println!("{}", r),
        _ => {}
    }
}

fn guard_ctor() {
    match Some(make()) {
        Some(r) if r.len() > 3 => println!("{}", r),
        _ => {}
    }
}

fn if_let_else() {
    if let Ok(s) = make_res() {
        println!("{}", s);
    } else {
        println!("no");
    }
}

fn if_let_no_else() {
    if let Ok(s) = make_res() {
        println!("{}", s);
    }
    println!("after");
}

// A partial move: the struct literal's temporary keeps b.
fn struct_literal(x: Option<String>) {
    if let P { a: Some(s), .. } = (P { a: x, b: make() }) {
        println!("{}", s);
    }
}

// Both fields move out: the second empties it.
fn both_fields() {
    match make_q() {
        Q { name: Some(s), tag } => println!("{} {}", s, tag),
        _ => {}
    }
}

// A tuple literal of calls, and a Copy part.
fn tuple_literal() {
    match ((make(), 1), make()) {
        ((a, n), b) if n > 0 => println!("{} {}", a, b),
        _ => {}
    }
}

// `ref` bindings borrow from the temporary.
fn ref_binding() {
    if let Ok(ref s) = make_res() {
        println!("{}", s);
    } else {
        println!("no");
    }
}

fn ref_mut_ctor() {
    match Some(make()) {
        Some(ref mut r) => r.push('x'),
        None => {}
    }
}

// Every arm that reaches the end of the match took everything: nothing is
// left to drop there.
fn in_loop() {
    loop {
        match make_res() {
            Ok(s) => println!("{}", s),
            Err(_) => break,
        }
    }
}

// The inner match starts on the outer arm's line.
fn nested_same_line() {
    match make_opt() {
        Some(r) if r.len() > 1 => match make_res() {
            Ok(s) => println!("{} {}", r, s),
            Err(_) => {}
        },
        _ => {}
    }
}

fn while_let(mut n: u32) {
    while let Ok(s) = make_res() {
        println!("{}", s);
        n -= 1;
        if n == 0 {
            break;
        }
    }
}

fn main() {
    guard_call();
    guard_ctor();
    if_let_else();
    if_let_no_else();
    struct_literal(Some(String::from("x")));
    both_fields();
    tuple_literal();
    ref_binding();
    ref_mut_ctor();
    in_loop();
    nested_same_line();
    while_let(2);
}
