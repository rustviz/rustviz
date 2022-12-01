fn main() {
    
    let x = String::from("World");
    
    let len_x = || x.len();
    
    let print_x = || println!("{}", x);

    println!("{}",len_x());
    
    print_x();
    
}