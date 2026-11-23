// A struct or tuple a call returns whose reference field borrows a string
// literal argument (`part("a")`): the field gets its value on the `let`
// line, like `Part { text: "a" }` does, and its borrow's end says it
// borrowed a string literal. It used to get no event there, so its first
// read landed on a column that never started (State::Invalid, #1356).
struct Part<'p> { text: &'p str }
struct Two<'p> { a: &'p str, b: &'p str }
struct O<'p> { inner: Part<'p>, n: i32 }
struct Mk;
impl Mk {
    fn part<'a>(&self, c: &'a str) -> Part<'a> { Part { text: c } }
}
fn part<'a>(c: &'a str) -> Part<'a> { Part { text: c } }
fn pair<'a>(c: &'a str) -> (&'a str, i32) { (c, 1) }
fn two<'a>(c: &'a str, d: &'a str) -> Two<'a> { Two { a: c, b: d } }
fn two2<'a, 'b>(c: &'a str, d: &'b str) -> (&'a str, &'b str) { (c, d) }
fn opart<'a>(c: &'a str) -> Option<(&'a str, i32)> { Some((c, 1)) }
fn get() -> &'static str { "g" }
fn lets() {
    let p = part("a");
    println!("{}", p.text);
    let q = pair("b");
    println!("{}", q.0);
    let t = two("c", "d");
    println!("{} {}", t.a, t.b);
}
fn through_unwrap() {
    let u = opart("e").unwrap();
    println!("{}", u.0);
}
fn through_try() -> Option<i32> {
    let v = opart("f")?;
    println!("{}", v.0);
    Some(v.1)
}
fn reassigned() {
    let mut p = part("a");
    println!("{}", p.text);
    p = part("z");
    println!("{}", p.text);
}
fn nested() {
    let o = O { inner: part("a"), n: 1 };
    println!("{} {}", o.inner.text, o.n);
}
fn untracked_arg() {
    let p = part(get());
    println!("{}", p.text);
}
fn mixed() {
    let s = String::from("s");
    let w = two2(&s, "lit");
    println!("{} {}", w.0, w.1);
}
fn branches(c: bool, n: i32) {
    let p = if c { part("a") } else { part("b") };
    println!("{}", p.text);
    let m = match n { 0 => part("c"), _ => part("d") };
    println!("{}", m.text);
}
fn method() {
    let mk = Mk;
    let p = mk.part("a");
    println!("{}", p.text);
}
fn main() {
    lets();
    through_unwrap();
    through_try();
    reassigned();
    nested();
    untracked_arg();
    mixed();
    branches(true, 0);
    method();
}
