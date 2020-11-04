fn main() {
    let x = String::from("hello");
    f(&x); 
    println!("{}", x)
}

fn f(s : &String) {
    println!("{}", *s)
}