fn run(r: &String, b: bool) {
    let r2 = &*r;
    if b {
    } else {
        println!("{}", r2);
    }
}
fn main() {}
