struct Pair {
    a: String,
    b: String,
}

impl Pair {
    fn into_a(self) -> String {
        self.a
    }
}

fn main() {
    let p = Pair { a: String::from("abc"), b: String::from("ab") };
    let s5;
    if true {
        s5 = p.into_a();
    }
}
