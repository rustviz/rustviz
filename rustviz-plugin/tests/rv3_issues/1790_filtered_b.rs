fn filtered(a: i32) {
    let o = Some(&a).filter(|_| false);
    println!("{o:?}");
}
