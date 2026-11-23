// #1310: a struct (or tuple) literal whose reference field gets a copy of
// another reference (`Excerpt { p: first }`), with the struct never used
// again. The struct has no `Drop` impl, so dropping it doesn't use the
// borrow: NLL ends it at the literal (rustc accepts `n.push('x')` right
// after it). main drew i.p's borrow, and n's, to the end of the function.
// With `impl Drop` the drop does use it, and it lasts to the scope end.
struct Excerpt<'a> {
    p: &'a str,
}
struct Held<'a> {
    p: &'a str,
}
impl Drop for Held<'_> {
    fn drop(&mut self) {}
}
fn copied() {
    let n = String::from("Ok. I'm fine.");
    let first = n.as_str();
    let i = Excerpt { p: first };
    let m = 1;
    println!("{}", m);
}
fn read_later() {
    let n2 = String::from("Ok.");
    let first2 = n2.as_str();
    let i2 = Excerpt { p: first2 };
    let m2 = 1;
    println!("{} {}", m2, i2.p);
}
fn with_drop() {
    let n3 = String::from("Ok.");
    let first3 = n3.as_str();
    let h = Held { p: first3 };
    let m3 = 1;
    println!("{}", m3);
}
fn tuple() {
    let n4 = String::from("Ok.");
    let first4 = n4.as_str();
    let t = (first4, 1);
    let m4 = 1;
    println!("{}", m4);
}
fn assign_later() {
    let n5 = String::from("Ok.");
    let first5 = n5.as_str();
    let mut i5 = Excerpt { p: "x" };
    i5 = Excerpt { p: first5 };
    let m5 = 1;
    println!("{}", m5);
}
fn field_copy() {
    let n6 = String::from("Ok.");
    let first6 = n6.as_str();
    let i6 = Excerpt { p: first6 };
    let j6 = Excerpt { p: i6.p };
    println!("{}", first6);
}
fn main() {
    copied(); read_later(); with_drop(); tuple(); assign_later(); field_copy();
}
