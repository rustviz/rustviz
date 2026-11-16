fn check(r: &String) -> bool {
    r.len() > 0
}

fn skipped() {
    let s = String::from("hello");
    let t = String::from("hi");
    let y = Some(1);
    let mut r = &t;
    if let Some(_n) = y && { r = &s; r.len() > 3 } {
        println!("long");
    }
    println!("{}", r);
}

fn unused_in_else() {
    let a = String::from("hello");
    let b = String::from("hi");
    let mut q = &b;
    if let true = check(q) && { q = &a; q.len() > 3 } {
        println!("{}", q);
    } else {
        println!("no");
    }
}

fn main() {
    skipped();
    unused_in_else();
}
