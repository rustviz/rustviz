fn main() {
    let mut s = String::from("hello");
    let r = &mut s;
    add_world(r);
    add_world(r);
    println!("{}", s);
}

fn add_world(t: &mut String) {
    t.push_str(", world");
}
