// Reading a `const` or a `static` (#690). A constant has no place in
// memory of its own: each use is a new copy of its value, so `n` and `k`
// get copies of LIMIT and the constant has no column. (`v` gets a new
// Vec from EMPTY: Vec isn't Copy, and its tooltip doesn't name the
// constant.) A static is one place that exists for the whole program:
// it gets a column in the function, and `let g = GREETING;` copies the
// reference it holds out of it.
const LIMIT: i32 = 10;
const EMPTY: Vec<i32> = Vec::new();
static GREETING: &str = "hi";

fn main() {
    let n = LIMIT;
    let mut k = 0;
    println!("{}", k);
    k = LIMIT;
    let g = GREETING;
    let v = EMPTY;
    println!("{} {} {} {:?}", n, k, g, v);
}
