// The `else` declares `t` and panics: the panic unwinds and drops `t`,
// then `s`, right there (#630), not at the `else`'s closing brace,
// which it never reaches.

fn f(c: bool) -> usize {
    let s = String::from("s");
    if c {
        println!("ok");
    } else {
        let t = String::from("t");
        panic!("bad {}", t);
    }
    s.len()
}

fn main() {
    println!("{}", f(true));
}
