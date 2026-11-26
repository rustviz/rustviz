fn downgrade(s: &mut String) -> &String { s }
fn main() {
    let mut s2 = String::from("rust");
    let r3 = downgrade(&mut s2);
    println!("{}", r3);
}
