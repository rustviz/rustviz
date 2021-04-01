fn main() {
    let mut x = String::from("Hello");
    let y = &mut x;
    let z = &x; // NOT OK, y is alive
    world(y); 
    f(z)
}

fn f(s : &String) {
    println!("Length: {}", s.len())
}

fn world(s : &mut String) {
    s.push_str(", world")
}
