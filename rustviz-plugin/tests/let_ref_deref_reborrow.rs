fn main() {
    let s = String::from("s");
    let r = &s;
    let ref d = *r;
    println!("{}", d);
}
