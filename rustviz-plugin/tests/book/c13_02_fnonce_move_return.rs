fn main() {
    let s = String::from("a");
    let f = move || s;
    let t = f();
    println!("{}", t);
}
