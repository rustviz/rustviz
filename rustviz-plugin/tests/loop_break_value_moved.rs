// `let r = loop { … break k; }`: `break k` moves `k`'s value into `r`
// (#431). `r` owns it from the `break` on and drops it at the end of
// `main`; `k` isn't dropped at the `break`.

fn main() {
    let r = loop {
        let k = String::from("k");
        break k;
    };
    println!("{}", r);
}
