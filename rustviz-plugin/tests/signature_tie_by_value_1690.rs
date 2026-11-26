use std::fmt::Display;

fn boxed<'b, T: Display + 'b>(t: T) -> Box<dyn Display + 'b> {
    Box::new(t)
}

fn pick<'a>(o: Option<&'a String>, d: &'a String) -> &'a String {
    o.unwrap_or(d)
}

// unwrap / unwrap_or take o by value: r's lifetime is that of the
// reference inside o, not o's own (r stays usable after o is gone:
// `let r; { let o = Some(&a); r = o.unwrap(); } println!("{r}");`
// compiles).
fn by_value_receiver() {
    let a = 1;
    let b = 2;
    let o = Some(&a);
    let r = o.unwrap();
    println!("{r}");
    let p = Some(&a).filter(|x| **x > 0);
    let q = p.unwrap_or(&b);
    println!("{q}");
}

fn type_bound_and_mixed() {
    let s = String::from("a");
    let t = String::from("b");
    let b = boxed(&s);
    println!("{b}");
    let r = pick(Some(&s), &t);
    println!("{r}");
}
