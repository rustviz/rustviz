fn id(s: String) -> String {
    s
}

fn main() {
    let s1 = String::from("a");
    let s2 = String::from("b");
    let a = id(s1);
    let b = id(s2);
    println!("{} {}", a, b);
}
