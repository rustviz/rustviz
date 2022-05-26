fn main() {
    let mut x = String::from("hello");
    let y = &mut x;

    f(&x);
    
    String::push_str(y,", world");

}

fn f(x : &String) { 
    println!("{}",x);
}