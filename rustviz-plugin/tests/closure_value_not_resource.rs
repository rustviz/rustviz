fn apply<F: Fn()>(f: F) { f() }
fn show(_s: &String) {}

fn main() {
    let s = String::from("hi");
    let f = || show(&s);
    let g = || f();
    g();
    let pr = || println!("{}", s);
    apply(&pr);
    let mut v = vec![1];
    let mut push = || v.push(2);
    let o = Some(1);
    o.map(|_| push());
}
