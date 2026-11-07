fn main() {
    let v = vec![1, 2, 3];
    let s = String::from("hi");
    let f = if v.len() > 2 { v.first().unwrap() } else { &0 };
    let t: &str = if f > &1 { s.as_str() } else { "" };
    println!("{} {}", f, t);
}
