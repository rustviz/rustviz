struct Counter {
    n: u32,
}

impl Counter {
    fn reset(&mut self) {
        self.n = 0;
    }
}

fn main() {
    let mut c = Counter { n: 5 };
    c.reset();
    println!("{}", c.n);
}
