fn main(){
    let mut x = String::from("Hello, ");
    
    let mut f = |y:&String| x.push_str(y);
    
    let world = String::from("World");
    
    f(&world);
    
    println!("{}",x);
}