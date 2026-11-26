struct Q<'a> { a: &'a String, n: i32 }
fn same_row() {
    let mut p = (String::from("a"), String::from("b"));
    let mut h = Q { a: &p.0, n: 0 }; let other = &p.1;
    p.0.push('x');
    h = Q { a: other, n: 1 };
    println!("{} {}", h.a, h.n);
}
