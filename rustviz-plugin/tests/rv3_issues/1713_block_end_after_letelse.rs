fn f(o: Option<i32>) {
    let s = String::new();
    {
        let k = String::from("k");
        let Some(x) = o else { return }; }
    println!("{}", s);
}
