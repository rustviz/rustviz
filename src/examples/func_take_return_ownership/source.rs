fn take_and_return_ownership(some_string : String) -> String {
    println!("{}", some_string);
    some_string
}
  
fn main() {
    let mut s = String::from("hello");
    s = take_and_return_ownership(s);
    println!("{}", s);   // OK
}