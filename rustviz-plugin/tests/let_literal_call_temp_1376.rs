struct Q {
    s: String,
}

fn mk() -> Q {
    Q { s: String::from("q") }
}

struct R {
    a: i32,
    b: Q,
}

fn tuple_and_branches() {
    let (a, _) = (1, mk());
    let c = a > 0;
    let (b, _) = if c { (1, mk()) } else { (2, mk()) };
    println!("{} {}", a, b);
}

fn struct_literal() {
    let R { a, .. } = R { a: 1, b: mk() };
    println!("{}", a);
}

fn ref_binding() {
    let (ref a, _) = (1, mk());
    println!("{}", a);
}

fn main() {
    tuple_and_branches();
    struct_literal();
    ref_binding();
}
