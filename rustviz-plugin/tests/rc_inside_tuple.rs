use std::rc::Rc;
fn take(t: (Rc<i32>, i32)) -> i32 {
    t.1
}
fn main() {
    let a = Rc::new(1);
    let n = take((Rc::clone(&a), 3));
    println!("{}", n);
}
