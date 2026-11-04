fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let x = add(add(1, 2), 3);
    println!("{}", x);
}
