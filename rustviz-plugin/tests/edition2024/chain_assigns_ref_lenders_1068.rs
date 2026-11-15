fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}

fn main() {
    let a = String::from("a");
    let b = String::from("bb");
    let s = String::from("hello");
    let y = Some(1);
    let mut r = longest(&a, &b);
    if let Some(_n) = y && { r = &s; r.len() > 3 } {
        println!("long {}", r);
    } else {
        println!("short {}", r);
    }
    tied(y);
}

fn tied(y: Option<i32>) {
    let c = String::from("c");
    let d = String::from("dd");
    let t = String::from("hi");
    let mut q: &str = &t;
    if let Some(_n) = y && { q = longest(&c, &d); q.len() > 3 } {
        println!("long {}", q);
    } else {
        println!("short {}", q);
    }
    println!("{}", q);
}
