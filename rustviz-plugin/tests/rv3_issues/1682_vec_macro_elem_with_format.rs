fn main() {
    let r = String::from("r");
    let s2 = String::from("ab");
    let v = vec![s2, format!("{}!", r)];
    println!("{}", v.len());
}
