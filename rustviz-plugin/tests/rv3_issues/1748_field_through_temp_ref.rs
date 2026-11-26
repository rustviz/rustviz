struct P { a: String, n: i32 }
fn f(mut p: P) {
    let r = &(&p).a;
    println!("{}", r);
}
