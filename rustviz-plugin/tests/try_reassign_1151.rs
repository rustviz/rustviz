// A `?` on the right of an assignment (#1151): if it returns early, the
// assignment never runs and the old value is dropped by the return; if
// it succeeds, the assignment drops the old value. Each function has one
// `?`, so each variable's `?` dot is about that line only.

fn bar() -> Result<String, ()> {
    Ok(String::from("b"))
}

fn maybe() -> Option<String> {
    Some(String::from("m"))
}

struct P {
    f: String,
}

fn plain() -> Result<(), ()> {
    let mut s = String::from("a");
    s = bar()?;
    println!("{}", s);
    Ok(())
}

fn in_branch(r: Result<String, ()>, c: bool) -> Result<(), ()> {
    let mut t = String::from("t");
    if c {
        t = r?;
    }
    println!("{}", t);
    Ok(())
}

fn field() -> Option<()> {
    let mut p = P { f: String::from("f") };
    p.f = maybe()?;
    println!("{}", p.f);
    Some(())
}

fn element() -> Option<()> {
    let mut v = vec![String::new()];
    v[0] = maybe()?;
    println!("{:?}", v);
    Some(())
}

fn through_ref() -> Option<()> {
    let mut u = String::from("u");
    let w = &mut u;
    *w = maybe()?;
    println!("{}", u);
    Some(())
}

fn borrowed() -> Result<(), ()> {
    let mut x = String::from("x");
    x.push_str(&bar()?);
    println!("{}", x);
    Ok(())
}

fn moved() -> Result<(), ()> {
    let y = String::from("y");
    let pair = (y, bar()?);
    println!("{:?}", pair);
    Ok(())
}

fn main() {
    let _ = plain();
    let _ = in_branch(Ok(String::new()), true);
    let _ = field();
    let _ = element();
    let _ = through_ref();
    let _ = borrowed();
    let _ = moved();
}
