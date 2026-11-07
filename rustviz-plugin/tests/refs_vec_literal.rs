fn main() {
    let s = String::from("s");
    let refs = vec![&s];
    let r = refs[0];
    println!("{}", r);
}
