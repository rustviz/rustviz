fn run(c: bool) {
    let s = String::from("ab");
    let a = &s;
    let r2 = a;
    if c {
    } else {
        println!("{}", r2);
    }
}
fn main() {}
