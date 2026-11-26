fn main() {
    let old = String::from("a");
    let f = move || { drop(old); String::new() };
    let x = f();
    println!("{}", x);
}
