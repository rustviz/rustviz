// `r?` is `match Try::branch(r) { … }`, and `Try::branch` takes `r` by
// value: a non-`Copy` `r` is moved by the `?` on both paths (#652). On
// `Ok` / `Some` the value inside goes on (into `n`, or to whatever uses
// `r?`); on `Err` / `None` the function returns early and the error goes
// to the caller, so `r` isn't dropped there. A `Copy` `Result<i32, i32>`
// is copied, and stays usable.

fn int_ok(r: Result<i32, String>) -> Result<i32, String> {
    let n = r?;
    Ok(n)
}

fn string_ok(rs: Result<String, String>) -> Result<usize, String> {
    let s = rs?;
    Ok(s.len())
}

fn option(o: Option<String>) -> Option<usize> {
    let t = o?;
    Some(t.len())
}

fn copy_result(c: Result<i32, i32>) -> Result<i32, i32> {
    let m = c?;
    Ok(m + c.unwrap_or(0))
}

fn unbound(e: Result<i32, String>) -> Result<i32, String> {
    Ok(e? + 1)
}

fn main() {
    let _ = int_ok(Ok(1));
    let _ = string_ok(Ok(String::from("a")));
    let _ = option(None);
    let _ = copy_result(Err(2));
    let _ = unbound(Ok(3));
}
