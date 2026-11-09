// #754: the same with `return x;`.
fn id(x: &String) -> &String {
    return x;
}

fn main() {
    let s = String::from("a");
    let r = id(&s);
    println!("{}", r);
}
