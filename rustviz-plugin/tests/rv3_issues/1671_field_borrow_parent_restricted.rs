struct P { a: String, n: i32 }
fn main() {
    let mut p = P { a: String::from("a"), n: 1 };
    let r = &mut p.a;
    r.push('!');
    println!("{}", p.n);
    println!("{}", r);
}
