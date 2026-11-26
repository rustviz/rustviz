struct P { name: String, id: String }
fn f(mut x: P, k: u8) {
    let mut n = 0;
    while n < k {
        x.name = String::new();
        if n == 0 {
            drop(x.name);
        }
        n += 1;
    }
}
