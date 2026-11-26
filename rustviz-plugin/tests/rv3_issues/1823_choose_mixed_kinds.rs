fn choose<'a>(x: &'a mut String, y: &'a String, flag: bool) -> &'a String {
    if flag { x } else { y }
}
fn f(flag: bool) {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = Vec::new();
    v.push(choose(&mut a, &b, flag));
    println!("{:?}", v);
}
