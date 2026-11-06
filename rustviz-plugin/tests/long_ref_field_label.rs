struct E<'a> { part: &'a str, n: i32 }
struct H<'a> { e: E<'a>, k: i32 }

fn main() {
    let s = String::from("hi");
    let h = H { e: E { part: &s, n: 3 }, k: 4 };
    println!("{} {}", h.e.part, h.e.n + h.k);
}
