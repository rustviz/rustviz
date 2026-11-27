fn run(o: Option<String>, flag: bool) -> Option<usize> {
    let s3 = String::from("x");
    let r = o.as_ref().unwrap();
    if let Some(x) = &o && x.len() > 1 && flag {
        let Some(x2) = o else { return None; };
    } else {
        println!("{}", r);
    }
    Some(0)
}
