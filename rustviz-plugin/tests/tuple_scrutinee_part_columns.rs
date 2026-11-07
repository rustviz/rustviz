struct P {
    a: String,
    b: String,
}

fn whole() {
    let s = String::from("s");
    let u = (String::from("u0"), String::from("u1"));
    match (s, u) {
        (p, _) => println!("{}", p),
    }
}

fn part() {
    let s2 = String::from("s2");
    let v = (String::from("v0"), String::from("v1"));
    match (s2, v) {
        (p2, (x2, _)) => println!("{} {}", p2, x2),
    }
}

fn fields() {
    let s3 = String::from("s3");
    let w = P { a: String::from("a"), b: String::from("b") };
    match (s3, w) {
        (p3, P { a, .. }) => println!("{} {}", p3, a),
    }
}

fn part_if_let() {
    let s4 = String::from("s4");
    let m = (String::from("m0"), String::from("m1"));
    if let (p4, (x4, _)) = (s4, m) {
        println!("{} {}", p4, x4);
    }
}

fn main() {
    whole();
    part();
    fields();
    part_if_let();
}
