fn push(mut x: String, y: String, c: bool, d: bool) {
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

fn assign(mut a: String, b: String, c: bool, d: bool) {
    let mut q = &a;
    match c {
        true => {
            a = String::new();
            let true = d else { return };
            q = &b;
            println!("{}", q);
        }
        false => println!("{}", q),
    }
}
