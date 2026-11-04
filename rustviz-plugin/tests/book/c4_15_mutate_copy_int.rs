fn add_one(mut x: i32) -> i32 {
    x += 1;
    x
}

fn main() {
    let a = 5;
    let b = add_one(a);
    println!("{} {}", a, b);
}
