fn main() {
    let mut x = String::from("Hello");
    let y = &x;
    x = String::from("Hi"); //NOT OK
    f(y)
}

fn f(s : &String) {
    println!("Length: {}", s.len())
}
