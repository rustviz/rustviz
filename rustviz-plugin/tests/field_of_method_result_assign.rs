struct Inner { n: i32 }
struct P { n: i32, s: String, inner: Inner }

fn main() {
    let mut v = vec![P { n: 1, s: String::from("a"), inner: Inner { n: 0 } }];
    v.last_mut().unwrap().n = 5;
    let t = String::from("b");
    v.last_mut().unwrap().s = t;
    v.first_mut().unwrap().n += 1;
    v.last_mut().unwrap().inner.n = 7;
    println!("{}", v.len());
}
