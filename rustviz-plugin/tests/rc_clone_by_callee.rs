use std::rc::Rc;

struct Maker;

impl Maker {
    // Not `Clone::clone`: only shares its name with it.
    fn clone(&self) -> Rc<i32> {
        Rc::new(1)
    }
}

fn main() {
    let m = Maker;
    let r = m.clone();
    let s = String::from("hi");
    let t = s.clone();
    let a = Rc::new(2);
    drop(a);
    println!("{} {}", r, t);
}
