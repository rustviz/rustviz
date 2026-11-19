// Calling an `FnOnce` closure moves it into the call (#271): what it
// captured is dropped or moved out there, and the closure holds nothing
// after. A `?` after the call that returns early has nothing of it to
// drop (#1136). The call is a statement before the `?`, the `?`'s own
// operand (`g()?`, which consumes g before the `?` looks at the result),
// in the same `if` as the `?`, or in an `if` before it (then the closure
// may or may not still hold s: the drop flag decides, and no dot claims
// either). `k` is an `Fn` closure (it only reads what it captured), so
// calling it doesn't use it up: its `?` dot stays.

fn bar() -> Result<(), ()> {
    Ok(())
}

fn direct() -> Result<(), ()> {
    let s = String::from("s");
    let f = move || drop(s);
    f();
    bar()?;
    Ok(())
}

fn operand() -> Result<usize, ()> {
    let t = String::from("t");
    let g = move || -> Result<String, ()> { Ok(t) };
    let q = String::from("q");
    let k = move || -> Result<usize, ()> { Ok(q.len()) };
    let u = g()?;
    let n = k()?;
    bar()?;
    Ok(u.len() + n)
}

fn same_branch(c: bool) -> Result<(), ()> {
    let v = String::from("v");
    let h = move || drop(v);
    if c {
        h();
        bar()?;
    }
    Ok(())
}

fn earlier_branch(c: bool) -> Result<(), ()> {
    let w = String::from("w");
    let m = move || drop(w);
    if c {
        m();
    }
    bar()?;
    Ok(())
}

fn main() {
    let _ = direct();
    let _ = operand();
    let _ = same_branch(true);
    let _ = earlier_branch(true);
}
