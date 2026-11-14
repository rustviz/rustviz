// A match arm with no events for `name` (the `None` arm) is dashed only
// beside the other arm's code, and solid over its own line (#843).
fn main() {
    let name = Some(String::from("Ferris"));
    match &name {
        Some(n) => println!("hello, {}", n),

        None => println!("no name"),
    }
    println!("{:?}", name);
}
