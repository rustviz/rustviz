// #232: a variable named like the fn that initializes it. The fn and
// the variable used to share one RAP (RAPs were keyed by bare name),
// so `area` the variable got no column and no events at all.
struct Rectangle {
    width: u32,
    height: u32,
}

fn area(rectangle: &Rectangle) -> u32 {
    rectangle.width * rectangle.height
}

fn main() {
    let rect = Rectangle { width: 30, height: 50 };
    let area = area(&rect);
    println!("{}", area);
}
