// #547: a Copy value produced by a match / if / block / call has no
// named source; its tooltip must not call it "Anonymous resource".
fn main() {
    let c = String::from("hello");
    let n = match Some(c) {
        Some(x) => x.len(),
        None => 0,
    };
    let flag = true;
    let m = if flag { n + 1 } else { n };
    let k = { let t = m * 2; t };
    println!("{} {} {}", n, m, k);
}
