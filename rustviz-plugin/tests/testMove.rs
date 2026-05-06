fn takes_and_drops(a: String) {
    println!("Inside function: {}", a);
}

fn main() {
    let s = String::from("hello");
    takes_and_drops(s);
    // println!("In main: {}", s);  // This will not compile
}