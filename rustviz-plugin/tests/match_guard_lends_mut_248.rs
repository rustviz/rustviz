fn main() {
    let mut v = vec![1];
    let opt = Some(5);
    match opt {
        Some(n) if { v.push(n); v.len() > 2 } => println!("long {}", n),
        _ => println!("other"),
    }
    println!("{:?}", v);
}
