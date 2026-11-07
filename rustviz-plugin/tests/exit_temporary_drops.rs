// A temporary that nothing binds (#457) and a `return` (#410). In `f`,
// `make()`'s String was dropped at the end of its statement, before
// the `return`, which drops only `s`. In `g`, the `return` leaves
// while `parse(&s)`'s Result is still alive, so it's dropped there.

fn make() -> String {
    String::from("a")
}

fn parse(s: &String) -> Result<u32, String> {
    if s.is_empty() {
        Err(String::from("empty"))
    } else {
        Ok(1)
    }
}

fn f(x: u32) -> usize {
    let s = String::from("s");
    let n = make().len();
    match x {
        0 => return n,
        _ => {}
    }
    s.len()
}

fn g(s: String) -> u32 {
    let n = match parse(&s) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    n
}

fn main() {
    f(1);
    g(String::from("x"));
}
