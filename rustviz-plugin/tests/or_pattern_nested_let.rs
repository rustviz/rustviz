fn main() {
    let t = (String::from("a"), (String::from("b"), String::from("c")));
    let (a, (x, _) | (_, x)) = t;
    println!("{} {}", a, x);
}
