fn f(mut x: String, y: String, c: bool, d: bool) {
    let mut r = &x;
    if c {
        x.push('x');
        if d {
            r = &y;
            println!("{}", r);
        }
    } else {
        println!("{}", r);
    }
}
