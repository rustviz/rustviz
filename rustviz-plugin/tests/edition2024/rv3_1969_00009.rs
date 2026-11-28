struct Pair {
    a: String,
    b: String,
}

fn main() {
    let mut s = String::from("ab");
    let mut s2 = String::from("rust");
    let mut p = Pair { a: s, b: String::from("x") };
    println!("{}", s2);
    let c = move || s2;
    let s3;
    if true {
        s3 = String::from("x");
    } else {
        s3 = p.a;
    }
}
