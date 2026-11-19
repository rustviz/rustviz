// `let s = *b;` moves a Box's whole contents out; the Box keeps its heap
// allocation. `b = Box::new(..)` then frees that old allocation (only:
// the old contents are gone) and gives b a new Box, which is whole: if it
// is moved away (`drop(b)`, `let c = b;`), nothing is left for b's scope
// end (#1145). If its contents are moved out again, the scope end frees
// only the allocation, as before. The reassignment also happens inside
// an `if`.

fn take(_s: String) {}

fn reboxed_then_dropped() {
    let mut a = Box::new(String::from("a"));
    let _s = *a;
    a = Box::new(String::from("b"));
    drop(a);
}

fn reboxed_then_emptied() {
    let mut b = Box::new(String::from("a"));
    take(*b);
    b = Box::new(String::from("b"));
    take(*b);
}

fn reboxed_then_moved() {
    let mut d = Box::new(String::from("a"));
    take(*d);
    d = Box::new(String::from("b"));
    let e = d;
    println!("{}", e);
}

fn reboxed_in_branch(c: bool) {
    let mut f = Box::new(String::from("a"));
    let _s = *f;
    if c {
        f = Box::new(String::from("b"));
        println!("{}", f);
    }
}

fn reboxed_then_kept() {
    let mut g = Box::new(String::from("a"));
    let _s = *g;
    g = Box::new(String::from("b"));
    println!("{}", g);
}

fn main() {
    reboxed_then_dropped();
    reboxed_then_emptied();
    reboxed_then_moved();
    reboxed_in_branch(true);
    reboxed_then_kept();
}
