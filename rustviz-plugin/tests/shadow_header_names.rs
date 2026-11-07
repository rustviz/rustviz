// Numbered column headers (#308) for names that are also words or
// letters in the header text: `s`, `n`, `e`, `a`, `i`, `b`. Each is
// re-let once, so each gets a `x₂` header that names `x` and `x₂` several
// times; every name must come out as one clean code-styled name.
fn main() {
    let s = String::from("a");
    let s = s.len();
    let n = 1;
    let n = n + 1;
    let e = 2;
    let e = e + 1;
    let a = 3;
    let a = a + 1;
    let i = 4;
    let i = i + 1;
    let b = 5;
    let b = b + 1;
    println!("{} {} {} {} {} {}", s, n, e, a, i, b);
}
