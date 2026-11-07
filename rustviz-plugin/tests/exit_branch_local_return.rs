// A variable declared inside a branch that returns is dropped at the
// `return`, before the ones declared outside it (reverse declaration
// order): `x`, then `t` (#630). The branch's closing brace is never
// reached, so nothing is dropped there.

fn f(c: bool) -> usize {
    let t = String::from("t");
    if c {
        let x = String::from("x");
        return t.len() + x.len();
    } else {
        println!("no");
    }
    t.len()
}

fn main() {
    println!("{}", f(true));
}
