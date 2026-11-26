fn main() {
    let x = 1;
    let mut s = (&x, 2);
    let mut v: Vec<&&i32> = Vec::new();
    v.extend([&s.0]);
    s.1 = 3;
    println!("{}", v.len());
}
