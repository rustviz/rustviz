fn main() {
    let mut x = String::from("Hello");
    world(&mut x);
    println!("{}", x);
}

fn world(s : &mut String) {
    s.push_str(", world");
}