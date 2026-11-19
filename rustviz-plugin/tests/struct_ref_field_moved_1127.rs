// #1127: a struct holding a reference, moved into a call (`drop(e)`),
// takes its reference field with it: e.part's borrow ends there, while a
// copy of the reference taken earlier keeps the lender borrowed.
struct Excerpt<'a> { part: &'a str }
impl<'a> Excerpt<'a> {
    fn part(&self) -> &'a str { self.part }
}
struct Outer<'a> { ex: Excerpt<'a> }

fn copied_field() {
    let novel = String::from("Call me Ishmael.");
    let e = Excerpt { part: &novel };
    let p = e.part;
    drop(e);
    let _spacer = 1;
    println!("{}", p);
}

fn from_method() {
    let book = String::from("Some years ago.");
    let f = Excerpt { part: &book };
    let q = f.part();
    drop(f);
    println!("{}", q);
}

fn nested() {
    let text = String::from("Never mind how long.");
    let o = Outer { ex: Excerpt { part: &text } };
    let w = o.ex.part;
    drop(o);
    println!("{}", w);
}

fn main() {
    copied_field();
    from_method();
    nested();
}
