fn main() {
    let mut x = String::from("hello");
    let y = &mut x;
    f(&x);
    String::push_str(y,String::from(", world"));
}


fn f(s : &String) {
    println!("{}",s);
}