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
    let s = String::from("x");
    let mut p = Pair { a: String::from("rust"), b: String::from("ab") };
    let s2 = 'blk: {
        s
    };
    p = Pair { a: s2.to_string(), b: p.into_a() };
    let Pair { a: s3, b: s4 } = p;
}
