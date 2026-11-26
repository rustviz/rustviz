fn run(o: Option<String>, a: bool) {
    let s = String::from("s");
    let r = &s;
    let r2 = &s;
    if a {
    } else if let Some(x) = o {
        let n = r2.len() + x.len();
    }
    println!("{}", r);
}
