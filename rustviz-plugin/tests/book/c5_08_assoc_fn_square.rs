struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    fn square(size: u32) -> Self {
        Self { width: size, height: size }
    }
}

fn main() {
    let sq = Rectangle::square(3);
    println!("{} {}", sq.width, sq.height);
}
