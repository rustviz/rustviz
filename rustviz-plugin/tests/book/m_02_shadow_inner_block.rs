fn main() {
    let x = String::from("outer");
    {
        let x = String::from("inner");
        println!("{}", x);
    }
    println!("{}", x);
}
