// #1173: a struct literal (or tuple literal) moves what it is built from
// when its destination has no column per field: an enum's struct variant,
// a place behind a reference, an element, a vec!/array element, a
// constructor's argument.
enum Msg { Write { text: String }, Quit }
struct P { name: String }
fn take<T>(_t: T) {}
fn main() {
    let s = String::from("hi");
    let m = Msg::Write { text: s };
    let t = String::from("t");
    let mut p = P { name: String::new() };
    let r = &mut p;
    *r = P { name: t };
    let a = String::from("a");
    let pairs = vec![(a, 1)];
    let c = String::from("c");
    let ps = vec![P { name: c }];
    let a2 = String::from("a2");
    let mut n = Msg::Quit;
    n = Msg::Write { text: a2 };
    let a3 = String::from("a3");
    let arr = [(a3, 1)];
    let b4 = String::from("b4");
    take(vec![(b4, 2)]);
    let e5 = String::from("e5");
    take(Msg::Write { text: e5 });
    let f6 = String::from("f6");
    let o6 = Some(Msg::Write { text: f6 });
    println!("{} {} {} {}", pairs.len(), ps.len(), arr.len(), p.name);
    if let Msg::Write { text } = m { println!("{}", text); }
    if let Msg::Write { text } = n { println!("{}", text); }
    println!("{}", o6.is_some());
}
fn wrap() -> Option<P> {
    let d = String::from("d");
    Some(P { name: d })
}
