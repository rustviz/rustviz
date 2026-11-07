fn nested() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    match (a, (b, c)) {
        (p, (x, _)) => println!("{} {}", p, x),
    }
}

fn nested_if_let() {
    let d = String::from("d");
    let e = Some(String::from("e"));
    let f = String::from("f");
    if let (q, (Some(y), _)) = (d, (e, f)) {
        println!("{} {}", q, y);
    }
}

fn nested_whole() {
    let g = String::from("g");
    let h = String::from("h");
    let k = String::from("k");
    match (g, (h, k)) {
        (r, z) => println!("{} {}", r, z.0),
    }
}

fn rest_pattern() {
    let a3 = String::from("a3");
    let b3 = String::from("b3");
    let c3 = String::from("c3");
    match (a3, b3, c3) {
        (.., z3) => println!("{}", z3),
    }
}

fn main() {
    nested();
    nested_if_let();
    nested_whole();
    rest_pattern();
}
