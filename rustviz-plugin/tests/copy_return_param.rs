fn add_one(mut x: i32) -> i32 {
    x += 1;
    x
}

fn is_big(n: i32) -> bool {
    let big = n > 10;
    big
}

fn main() {
    let a = 5;
    let b = add_one(a);
    let c = is_big(b);
    println!("{} {} {}", a, b, c);
}
