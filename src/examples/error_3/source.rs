fn main() {
    let s = String::from("hello");
    let x = &s;
    let s2 = s;
    println!("{}", x);
}