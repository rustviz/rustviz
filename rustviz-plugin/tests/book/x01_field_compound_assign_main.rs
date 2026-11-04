struct Counter {
    n: u32,
}

fn main() {
    let mut c = Counter { n: 0 };
    c.n += 1;
    println!("{}", c.n);
}
