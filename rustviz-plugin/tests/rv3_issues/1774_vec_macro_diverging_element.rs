fn f(c: bool) -> usize {
    let s = String::from("s");
    if c {
        let v = vec![s, return 0];   // "Move from s to v"; v "acquires ownership"
        return v.len();
    }
    0
}
fn g() {
    let t = String::from("t");
    let pv = vec![panic!("stop"), t]; // "pv acquires ownership of a resource",
    println!("{}", pv.len());         // "pv goes out of scope. Its resource is dropped (including every String it holds)."
}
