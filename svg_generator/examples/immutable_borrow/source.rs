fn main() {
    let x = String::from("hello");
    let y = f(&x); 
    println!("{}", x)
}

fn f(s : &String) {
    println!("{}", s)
}