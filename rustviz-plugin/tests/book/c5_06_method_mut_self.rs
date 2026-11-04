struct Counter {
    n: u32,
}

impl Counter {
    fn inc(&mut self) {
        self.n += 1;
    }
}

fn main() {
    let mut c = Counter { n: 0 };
    c.inc();
    c.inc();
    println!("{}", c.n);
}
