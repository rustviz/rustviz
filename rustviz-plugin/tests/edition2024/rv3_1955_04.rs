struct Pair {
    a: String,
    b: String,
}

fn main() {
    let s = String::from("hey");
    let s2 = String::from("hey");
    let p = Pair { a: s, b: s2 };
    if true { return; }
    let Pair { a: s5, .. } = p;
}
