// The `?` operator returns early on `Err` / `None`, and that return
// drops every value the function still owns (#430). It happens only on
// that path, so each owner gets a dot on its column at the `?`, and the
// column goes on. `a` was moved into `take` before the first `?`, `n`
// is a `Copy`, and `opt` is one too: none of them is dropped there.

fn take(s: String) -> Option<usize> {
    Some(s.len())
}

fn run(a: String, b: String, opt: Option<usize>) -> Option<usize> {
    let c = String::from("c");
    let n = take(a)?;
    let m = opt?;
    let k = b.len();
    Some(n + m + k + c.len())
}

fn main() {
    println!("{:?}", run(String::from("a"), String::from("b"), Some(1)));
}
