struct P { n: String, k: i32 }
struct Q { k: i32 }
fn main() {
    let t = (P { n: String::from("a"), k: 1 }, P { n: String::from("b"), k: 2 });
    match t {
        (x, _) | (_, x) => println!("{} {}", x.n, x.k),
    }
    let u = (Q { k: 1 }, Q { k: 2 });
    match u {
        (y, _) | (_, y) => println!("{}", y.k),
    }
    let c = ((1, 2), (3, 4));
    match c {
        (z, (5, _)) | ((5, _), z) => println!("{}", z.0),
        _ => {}
    }
}
