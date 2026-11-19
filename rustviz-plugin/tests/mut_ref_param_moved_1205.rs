fn id<T>(x: T) -> T { x }
fn bump(r: &mut String) {
    let r2 = id(r);
    let _spacer = 1;
    r2.push('!');
}
fn main() {
    let mut s = String::from("hello");
    bump(&mut s);
}
