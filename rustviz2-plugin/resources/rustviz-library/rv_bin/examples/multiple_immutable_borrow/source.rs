fn main() {
    let x = String::from("hello");
    let y = &x;
    let z = &x;
    f(y, z);
}

fn f(s1 : &String, s2 : &String) {
    println!("{} and {}", s1, s2);
}