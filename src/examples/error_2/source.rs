fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", x) // ERROR: x does not own a resource
}