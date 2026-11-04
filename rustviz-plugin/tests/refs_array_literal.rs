fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let pair = [&a, &b];
    let first = pair[0];
    drop(pair);
    println!("{}", first);
}
