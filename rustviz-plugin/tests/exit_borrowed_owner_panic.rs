// #645: the same at a panic, which unwinds and drops `t`.
fn g(c: bool) -> usize {
    let t = String::from("t");
    if c {
        println!("yes");
    } else {
        let r = &t;
        panic!("{}", r);
    }
    t.len()
}

fn main() {
    println!("{}", g(true));
}
