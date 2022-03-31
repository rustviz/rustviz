fn main() {
    let x = String::from("hello");
    let z = {
        let y = x;
        println("{}", y);
        y
        // ...
    };
    println!("Hello, world!");
}