fn main() {
    let mut x = String::from("hello");
    let y = &mut x;
    f(&x); // ERROR: y is still live
    String::push_str(y, ", world");
}
  
fn f(x : &String) {
    println!("{}", x);
}