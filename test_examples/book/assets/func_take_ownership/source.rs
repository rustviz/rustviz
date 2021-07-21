fn main() {
    let s = String::from("hello");
    takes_ownership(s);
    // println!("{}", s) // won't compile if added
}

fn takes_ownership(some_string: String) {
    println!("{}", some_string)
}