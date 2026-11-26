fn f6() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r = &a;
    println!("{}", r);
    let n = r.len(); r = &b; let v = r.is_empty();
}
