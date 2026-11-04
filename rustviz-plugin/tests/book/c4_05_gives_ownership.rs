fn gives_ownership() -> String {
    let some_string = String::from("yours");
    some_string
}

fn main() {
    let s1 = gives_ownership();
    println!("{}", s1);
}
