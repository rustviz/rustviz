fn one_arm() {
    let a = String::from("a");
    let b = String::from("b");
    match (a, b) {
        (x, _) | (_, x) => println!("{}", x),
    }
}
fn three() {
    let a: Option<String> = None;
    let b = Some(String::from("b"));
    let c = String::from("c");
    match (a, b, c) {
        (Some(x), _, _) | (_, Some(x), _) => println!("{}", x),
        _ => println!("none"),
    }
}
fn main() {
    one_arm();
    three();
}
