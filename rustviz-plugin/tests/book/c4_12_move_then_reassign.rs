fn takes(s: String) {
    println!("{}", s);
}

fn main() {
    let mut s = String::from("a");
    takes(s);
    s = String::from("b");
    println!("{}", s);
}
