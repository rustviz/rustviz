fn run(flag: bool) -> Option<usize> {
    let s2 = String::from("hi");
    let mut s3 = String::from("ho");
    let mut vr = vec![s2.as_str()];
    loop {
        if vr.is_empty() { return None; }
        break;
    }
    let r2 = &s3;
    vr.push(r2.as_str());
    Some(0)
}
