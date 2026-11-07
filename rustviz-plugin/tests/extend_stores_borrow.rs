// `extend` stores the references it's given in the collection (#682).
// Its signature ties them to the collection only through a trait bound
// (`I: IntoIterator<Item = T>`), so rustc's own facts say what `refs`
// borrows once the call returns.
fn main() {
    let s = String::from("s");
    let mut refs: Vec<&String> = Vec::new();
    refs.extend([&s]);
    println!("{}", refs.len());
    let a = String::from("a");
    let r = &a;
    let mut more: Vec<&String> = Vec::new();
    more.extend(vec![r]);
    println!("{}", more.len());
    println!("{} {}", s, a);
}
