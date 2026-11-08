// `return x` moves `x` to the caller; `t` is dropped there. Nothing is
// stated for `x` at the branch's closing brace, which the returning
// path never reaches (#630). The `if let` binding `s` is dropped at its
// `return`.

fn f(c: bool) -> String {
    let t = String::from("t");
    if c {
        let x = String::from("x");
        return x;
    } else {
        println!("no");
    }
    t
}

fn g(o: Option<String>) -> usize {
    let u = String::from("u");
    if let Some(s) = o {
        return u.len() + s.len();
    } else {
        println!("none");
    }
    u.len()
}

fn main() {
    println!("{} {}", f(true), g(None));
}
