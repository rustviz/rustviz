fn main() { 
    let mut s1 = String::from("Hello");
    let s2 = String::from(", world");
    String::push_str(&mut s1, &s2); 
    s1.push_str(&s2); // shorthand for the above
    println!("{}", s1); // prints "Hello, world, world"
}