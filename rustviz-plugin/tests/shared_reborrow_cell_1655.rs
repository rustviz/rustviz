use std::cell::Cell;

fn run(r: &mut Cell<i32>) {
    let shared = &*r;
    r.set(1);
    assert_eq!(shared.get(), 1);
}

fn main() {
    run(&mut Cell::new(0));
}
