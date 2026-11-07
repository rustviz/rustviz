use std::rc::Rc;

fn consume(r: Rc<i32>) {}

fn main() {
    let a = Rc::new(1);
    let n = 3;
    if n > 2 {
        consume(a);
    } else {
        println!("{}", a);
    }
}
