use std::cell::Cell;

fn pick(a: &Cell<i32>) -> &Cell<i32> {
    a
}

fn main() {
    let c = Cell::new(1);
    let r = pick(&c);
    r.set(2);
    println!("{}", c.get());
}
