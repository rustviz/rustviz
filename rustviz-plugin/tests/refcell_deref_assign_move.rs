use std::cell::RefCell;

fn main() {
    let cell = RefCell::new(String::from("a"));
    let v = String::from("b");
    *cell.borrow_mut() = v;
    println!("{:?}", cell);
}
