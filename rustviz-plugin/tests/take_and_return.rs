// Function takes ownership of a String and gives it back; caller
// reassigns the same binding to the returned value.

fn take_and_return(s: String) -> String {
    println!("{}", s);
    s
}

fn main() {
    let mut s = String::from("hello");
    s = take_and_return(s);
    println!("{}", s);
}
