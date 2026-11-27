fn run(n: i32) {
    let mut s = String::from("x");
    let mut t = String::from("y");
    let mut r = &s;
    let x = &s;
    match n {
        0 => r = &t,
        _ => println!("{} {}", r, x),
    }
    println!("{}", r);
}
