// A shared reference made from a mutable borrow keeps the borrow mutable
// while it is live: s can't even be read until r's last use (E0502).
// RustViz drew it as a shared borrow, and s's line vanished (#1122).
fn downgrade(s: &mut String) -> &str { s.as_str() }
struct C { n: i32 }
impl C {
    fn bump(&mut self) -> &i32 { self.n += 1; &self.n }
}
fn first(v: &mut Vec<i32>) -> Option<&i32> { v.first() }
struct H<'a> { r: &'a str }
fn mk(s: &mut String) -> H<'_> { H { r: s.as_str() } }

fn call_result() {
    let mut a = String::from("a");
    let ra = downgrade(&mut a);
    println!("{}", ra);
    println!("{}", a);
}
fn coercion() {
    let mut b = String::from("b");
    let t = String::from("t");
    let mut rb: &String = &mut b;
    println!("{}", rb);
    println!("{}", rb);
    rb = &t;
    println!("{} {}", rb, b);
}
fn method() {
    let mut c = C { n: 0 };
    let rc = c.bump();
    println!("{}", rc);
    println!("{}", c.n);
}
fn assignment() {
    let mut d = String::from("d");
    let t = String::from("t");
    let mut rd: &str = &t;
    println!("{}", rd);
    rd = downgrade(&mut d);
    println!("{}", rd);
    println!("{}", d);
}
fn pattern() {
    let mut v = vec![1, 2];
    if let Some(x) = first(&mut v) {
        println!("{}", x);
    }
    println!("{:?}", v);
}
fn struct_field() {
    let mut e = String::from("e");
    let he = H { r: downgrade(&mut e) };
    println!("{}", he.r);
    let mut f = String::from("f");
    let hf = mk(&mut f);
    println!("{}", hf.r);
    println!("{} {}", e, f);
}
fn copied() {
    let mut g = String::from("g");
    let rg = downgrade(&mut g);
    let rg2 = rg;
    println!("{} {}", rg, rg2);
    println!("{}", g);
}
fn chain() {
    let mut w = vec![1, 2, 3];
    let xw: &i32 = w.last_mut().unwrap();
    println!("{}", xw);
    w.push(4);
}
fn main() {
    call_result();
    coercion();
    method();
    assignment();
    pattern();
    struct_field();
    copied();
    chain();
}
