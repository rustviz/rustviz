fn two(o: Option<i32>, p: Option<i32>) {
    let s = String::new();
    let Some(x) = o else { return }; let r = &p; let Some(y) = r else { return };
    println!("{}{}{}", x, y, s);
}
