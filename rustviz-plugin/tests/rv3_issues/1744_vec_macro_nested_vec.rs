fn main() {
    let s2 = String::from("ab");
    let v = vec![vec![s2], vec![format!("{}", 1)]];
    println!("{}", v.len());
}
