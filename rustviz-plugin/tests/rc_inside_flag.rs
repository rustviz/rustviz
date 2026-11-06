use std::rc::Rc;

fn main() {
    let a = Rc::new(1);
    let o = Some(Rc::clone(&a));
    let v = vec![Rc::clone(&a)];
    if *a > 0 {
        drop(o);
        drop(v);
    }
}
