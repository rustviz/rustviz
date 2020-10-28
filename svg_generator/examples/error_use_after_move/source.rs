fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", x) // error: x does not own a resource
}
