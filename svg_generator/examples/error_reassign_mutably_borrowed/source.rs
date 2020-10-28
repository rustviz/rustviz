fn main() {
    let mut x = String::from("Hello");
    let y = &mut x;
    x = String::from("Hi"); // NOT OK, y is still live
    world(y)
}

fn world(s : &mut String) {
    s.push_str(", world")
}
