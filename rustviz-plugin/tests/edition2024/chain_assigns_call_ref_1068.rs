fn first(v: &Vec<String>) -> &String {
    &v[0]
}

fn main() {
    let v = vec![String::from("hello")];
    let t = String::from("hi");
    let y = Some(1);
    let mut r = &t;
    if let Some(_n) = y && { r = first(&v); r.len() > 3 } {
        println!("long {}", r);
    } else {
        println!("short {}", r);
    }
    method(y);
    index(y);
}

fn method(y: Option<i32>) {
    let s = String::from("hello");
    let u = String::from("hi");
    let mut q: &str = u.as_str();
    if let Some(_n) = y && { q = s.as_str(); q.len() > 3 } {
        println!("long {}", q);
    } else {
        println!("short {}", q);
    }
}

fn index(y: Option<i32>) {
    let w = vec![String::from("hello")];
    let u = String::from("hi");
    let mut p = &u;
    if let Some(_n) = y && { p = &w[0]; p.len() > 3 } {
        println!("long {}", p);
    }
    println!("{}", p);
}
