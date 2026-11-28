struct Pair {
    a: String,
    b: String,
}

fn main() {
    let s = String::from("abc");
    let b = Box::new(s);
    let p = Pair { a: format!("{}!", b), b: *b };
}
