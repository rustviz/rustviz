struct P { a: String, b: String }
fn main() {
    let b = Box::new(P { a: String::from("a"), b: String::from("b") });
    let x = b.a;
    println!("{} {}", x, b.b);
}

fn consume(_: String) {}

fn conditional(mut pair: Box<P>, flag: bool, ok: Option<()>) -> Option<()> {
    if flag {
        consume(pair.a);
        println!("{}", pair.b);
    }
    ok?;
    *pair = P { a: String::new(), b: String::new() };
    println!("{}", pair.a);
    Some(())
}

struct Z;
impl Drop for Z {
    fn drop(&mut self) { println!("drop Z"); }
}
fn zero_sized() {
    let z = Box::new((Z, Z));
    let (first, _) = *z;
    drop(first);
}
