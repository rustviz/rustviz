struct P { a: String, b: String }
struct H { inner: Box<P> }
fn f(h: H) {
    let x = h.inner.a;
    println!("{}", x);
}
