fn holder_through(mut s: String) {
    let r: &String = &mut s;
    let o = Some(r);
    let mut v: Vec<Option<&String>> = Vec::new();
    v.push(o);
    println!("{:?}", v);
}
