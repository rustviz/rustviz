// #905: an `if` with no `else` whose body moves a Box's contents out
// (`let w = *e;`, `take(*e2)`). On the path that skips the body, e still
// owns its String, which is dropped at e's scope end: a drop flag
// decides, and the Box's heap allocation is freed either way.
fn take(s: String) {
    println!("{}", s);
}

fn main() {
    let e = Box::new(String::from("a"));
    let k = 3;
    if k == 0 {
        let w = *e;
        drop(w);
    }
    let e2 = Box::new(String::from("b"));
    if k > 1 {
        take(*e2);
    }
}
