// The result of an `FnOnce` call is a new value, not a copy of
// anything in the closure (#271): `k` isn't "copied from g".

fn main() {
    let v = vec![1, 2, 3];
    let g = move || {
        let w = v;
        w.len()
    };
    let k = g();
    println!("{}", k);
}
