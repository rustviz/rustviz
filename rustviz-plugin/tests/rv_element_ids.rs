fn takes(s: String) -> usize {
    s.len()
}

fn main() {
    let s = String::from("hi");
    let r = &s;
    println!("{}", r);
    let t = s;
    let n = takes(t);
    println!("{}", n);
}
