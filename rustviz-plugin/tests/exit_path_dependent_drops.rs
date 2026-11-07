// What a `return` drops depends on the path to it (#410):
// - `pick`: `s` gets its value from the other arm, which this path
//   doesn't take, so nothing of `s` is dropped at `_ => return 0`.
//   (It used to be: `consume`'s parameter, also named `s`, was taken
//   for it.)
// - `f`: the `else` returns after the `if` body moved `t`; on the
//   `else` path `t` still owns its String and is dropped there.
// - `g`: `m` was moved on one path above only, so whether the
//   `return` drops it depends on the path; no drop is drawn there
//   (Rust decides with a runtime drop flag).

fn consume(s: String) -> usize {
    s.len()
}

fn pick(n: i32) -> usize {
    let s = match n {
        0 => String::from("x"),
        _ => return 0,
    };
    consume(s)
}

fn f(t: String, c: bool) -> usize {
    if c {
        consume(t);
    } else {
        return 0;
    }
    1
}

fn g(c: bool, n: i32) -> usize {
    let m = String::from("m");
    if c {
        consume(m);
    } else {
        consume(String::new());
    }
    match n {
        0 => return 0,
        _ => {}
    }
    1
}

fn main() {
    println!("{} {} {}", pick(1), f(String::from("t"), true), g(true, 1));
}
