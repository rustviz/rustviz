fn main() {
    let mut x = String::from("Hello");
    let y = &mut x;
    world(y);
    let z = &mut x; // OK, because y's lifetime has ended (last use was on previous line)
    world(z);
    x.push_str("!!"); // Also OK, because y and z's lifetimes have ended
    println!("{}", x)
}

fn world(s : &mut String) {
    s.push_str(", world")
}