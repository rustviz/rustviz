fn run() {
    let mut o = Some(String::from("rust"));
    if let Some(x) = &o && x.len() > 1 { let m = o.as_mut().unwrap(); }
}
