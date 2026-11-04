fn main() {
    let a = String::from("a");
    {
        let b = String::from("b");
        let r = &a;
        println!("{} {}", r, b);
    }
    println!("{}", a);
}
