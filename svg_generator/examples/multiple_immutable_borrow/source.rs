fn main() {
    let x = String::from("hello");
    let y = &x;
    let z = &x;
    println!("{}", x);
    f(y, z)
}

fn f(s1 : &String, s2 : &String) {
    println!("Length: {} and {}", s1.len(), s2.len())
}
