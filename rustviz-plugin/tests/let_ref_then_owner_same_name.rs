fn main() {
    let s = String::from("a");
    {
        let x = &s;
        println!("{}", x);
    }
    {
        let x = String::from("c");
        println!("{}", x);
    }
}
