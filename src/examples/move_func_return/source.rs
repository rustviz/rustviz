fn f() -> String {
    let x = String::from("hello");
    // ...
    x
} 
  
fn main() {
    let s = f();
    println!("{}", s);
}