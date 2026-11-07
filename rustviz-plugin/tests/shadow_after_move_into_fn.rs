// Shadowing a variable that was moved into a call (#253).

fn take(_s: String) {}

fn main() {
    let s = String::from("a");
    take(s);
    let s = String::from("b");
    println!("{}", s);
}
