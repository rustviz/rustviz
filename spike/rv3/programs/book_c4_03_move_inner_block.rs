fn main() {
    let s1 = String::from("hello");
    {
        let s2 = s1;
        println!("{}", s2);
    }
    let x = 5;
    println!("{}", x);
}
