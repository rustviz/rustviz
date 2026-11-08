// `x` is declared in the `if` branch and dropped at both inner
// `return`s; every path through the branch returns, so its closing
// brace drops nothing (#630).

fn f(c: bool, d: bool) -> usize {
    let t = String::from("t");
    if c {
        let x = String::from("x");
        if d {
            return x.len();
        } else {
            return 2;
        }
    } else {
        println!("no");
    }
    t.len()
}

fn main() {
    println!("{}", f(true, false));
}
