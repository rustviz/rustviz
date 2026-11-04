struct Counter {
    n: i32,
}

impl Counter {
    fn get(&self) -> i32 {
        self.n
    }
}

fn main() {
    let c = Counter { n: 3 };
    let v = c.get();
    println!("{}", v);
}
