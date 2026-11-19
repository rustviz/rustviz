struct Holder<'a>(&'a String);
struct Pair<'a>(&'a String, String);
struct W<T>(T);
struct O<'a> { h: Holder<'a> }
fn main() {
    let x = String::from("x");
    let h = Holder(&x);
    println!("{}", h.0);
    let r = &x;
    let h2 = Holder(r);
    println!("{}", h2.0);
    let name = String::from("n");
    let p = Pair(&x, name);
    println!("{} {}", p.0, p.1);
    let o = O { h: Holder(&x) };
    println!("{}", o.h.0);
    let mut s = String::from("hello");
    let rm = &mut s;
    let w = W(rm);
    let _spacer = 1;
    w.0.push('!');
}
