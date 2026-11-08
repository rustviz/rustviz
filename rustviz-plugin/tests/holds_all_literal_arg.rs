fn main() {
    let s = String::from("hello");
    let t = String::from("world");
    let p = Some([&s, &t]);
    if let Some(a) = p {
        println!("{} {}", a[0], a[1]);
    }
}
