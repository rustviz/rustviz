// A reference field of a struct bound by a pattern (`x.part: &str`) is
// a reference, not an owner. It holds the reference the matched value
// held (here, a borrow of `s` made on line 10), so it borrows `s` (#692).
// `w` mixes a `&s` and a `&mut k`, which `w` holds with their own kinds (#708).
struct E<'a> { part: &'a str, n: i32 }
struct W<'a> { e: E<'a>, m: &'a mut i32 }

fn main() {
    let s = String::from("hi there");
    let o = Some(E { part: &s[3..], n: 2 });
    if let Some(x) = o {
        let q = x.part;
        println!("{} {}", q, x.n);
    }
    let mut k = 1;
    let w = Some(W { e: E { part: &s, n: 1 }, m: &mut k });
    match w {
        Some(y) => println!("{} {}", y.e.part, y.m),
        None => {}
    }
    println!("{}", k);
}
