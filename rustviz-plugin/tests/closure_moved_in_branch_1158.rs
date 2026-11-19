// #1158: a closure moved (not called) in a branch: its scope end's drop
// depends on whether a branch moved it.
fn pushed(c: bool) {
    let s = String::from("a");
    let f = move || println!("{}", s);
    let mut v = Vec::new();
    if c {
        v.push(f);
    }
    println!("{}", v.len());
}
fn called(c: bool) {
    let t = String::from("b");
    let g = move || drop(t);
    if c {
        g();
    }
}
fn main() {
    pushed(true);
    called(true);
}
