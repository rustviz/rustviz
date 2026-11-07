fn tuple_var() {
    let s = String::from("s");
    let u = (String::from("u0"), String::from("u1"));
    match (s, u) {
        (p, (x, _) | (_, x)) => println!("{} {}", p, x),
    }
}

fn nested_literal() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    match (a, (b, c)) {
        (q, (y, _) | (_, y)) => println!("{} {}", q, y),
    }
}

fn top_level() {
    let s2 = String::from("s2");
    let v = (String::from("v0"), String::from("v1"));
    match (s2, v) {
        (r, (z, _)) | (r, (_, z)) => println!("{} {}", r, z),
    }
}

fn if_let() {
    let s3 = String::from("s3");
    let w = (String::from("w0"), String::from("w1"));
    if let (o, (n, _) | (_, n)) = (s3, w) {
        println!("{} {}", o, n);
    }
}

fn main() {
    tuple_var();
    nested_literal();
    top_level();
    if_let();
}
