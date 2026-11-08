struct E<'a> { part: &'a str, n: i32 }
struct W<'a> { e: E<'a>, m: &'a mut i32 }
fn main() {
    let s = String::from("hi there");
    let mut k = 1;
    let w = Some(W { e: E { part: &s, n: 1 }, m: &mut k });
    println!("{}", s);
    match w {
        Some(y) => println!("{} {}", y.e.part, y.m),
        None => {}
    }
    println!("{}", k);
}
