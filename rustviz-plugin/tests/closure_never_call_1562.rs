// #1562: a call of a local closure whose body never completes, classified
// from the closure's body as a `!` function's is (#1553): one that always
// panics (`f`) unwinds, dropping what is in scope; one that ends the
// process (`q`) or loops forever (`w`) drops nothing; for `m` the body
// doesn't say, and the tooltip gives both outcomes. Before edition 2024
// rustc types these calls `()`. main drew none of them as leaving.
fn main() {
    let s = String::from("hey");
    let f = || panic!("x");
    if s.len() > 9 {
        f();
    }
    println!("{}", s);
    quits();
    spins();
    top();
    mixed(1);
    moved();
}
fn quits() {
    let s2 = String::from("hey");
    let q = || std::process::exit(1);
    if s2.len() > 9 {
        q();
    }
    println!("{}", s2);
}
fn spins() {
    let s3 = String::from("hey");
    let w = || loop {};
    if s3.len() > 9 {
        w();
    }
    println!("{}", s3);
}
fn top() {
    let s4 = String::from("hey");
    let g = || { panic!("y"); };
    g();
}
fn mixed(n: u32) {
    let s5 = String::from("hey");
    let m = |k: u32| if k > 3 { panic!("big") } else { loop {} };
    m(n);
}
fn moved() {
    let s6 = String::from("hey");
    let t = String::from("t");
    let h = move || { drop(t); panic!("z") };
    if s6.len() > 9 {
        h();
    }
    println!("{}", s6);
}
