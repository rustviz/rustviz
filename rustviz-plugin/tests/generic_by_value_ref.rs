fn id<T>(x: T) -> T {
    x
}
fn main() {
    let s = String::from("a");
    let r = id(&s);
    let o = Some(&s);
    println!("{} {:?}", r, o);
}
