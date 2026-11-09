fn take(a: [&String; 1]) -> usize {
    a[0].len()
}
fn main() {
    let s = String::from("s");
    let n = take([&s]);
    println!("{} {}", n, s);
}
