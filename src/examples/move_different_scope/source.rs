fn main() {
    let x = String::from("hello");
    let z = {
        let y = x;
        println("{}", y);
        // ...
    };
    println!("Hello, world!");
}