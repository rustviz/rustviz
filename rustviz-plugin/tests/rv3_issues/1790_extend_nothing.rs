fn extend_nothing() {
    let p = (1, 2);
    let mut v: Vec<&i32> = Vec::new();
    v.extend(Some(&p.0).filter(|_| false));
    println!("{}", v.len());
}
