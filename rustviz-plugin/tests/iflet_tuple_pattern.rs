fn main() {
    let x = Some(String::from("a"));
    let y = Some(String::from("b"));
    if let (Some(a), Some(b)) = (x, y) {
        println!("{} {}", a, b);
    }

    let p = Some(String::from("p"));
    let q = Some(String::from("q"));
    if let (Some(_), Some(r)) = (p, q) {
        println!("{}", r);
    } else {
        println!("none");
    }

    let entry = Some((String::from("k"), 1));
    if let Some((key, n)) = entry {
        println!("{} {}", key, n);
    }
}
