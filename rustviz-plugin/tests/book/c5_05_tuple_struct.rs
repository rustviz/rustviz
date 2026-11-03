struct Color(i32, i32, i32);

fn main() {
    let black = Color(0, 0, 0);
    let r = black.0;
    println!("{} {}", r, black.2);
}
