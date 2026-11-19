// #1195: arguments are evaluated left to right, so `s` is moved into the
// call's first argument before `bar()?` runs. If the `?` returns early the
// call never happens and s's String is dropped as the function returns.
// The same for a method call's argument and a receiver taken by value.
fn bar() -> Result<String, ()> { Ok(String::new()) }
fn take(_a: String, _b: String) {}
struct P { a: String }
impl P {
    fn put(&mut self, _a: String, _b: String) {}
    fn eat(self, _b: String) {}
}
fn demo() -> Result<(), ()> {
    let s = String::from("s");
    take(s, bar()?);
    let mut q = P { a: String::new() };
    let w = String::from("w");
    q.put(w, bar()?);
    q.eat(bar()?);
    Ok(())
}
fn main() {
    let _ = demo();
}
