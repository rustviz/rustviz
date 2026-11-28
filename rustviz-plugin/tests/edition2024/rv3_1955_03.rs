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
    if true { return; }
    let mut p2 = Pair { a: String::from("abc"), b: String::from("rust") };
    p2 = Pair { a: p2.into_a(), b: String::from("hey") };
}
