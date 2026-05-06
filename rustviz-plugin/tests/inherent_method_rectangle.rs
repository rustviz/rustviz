// Inherent method on a user struct (Rectangle/area pattern).
// Calling `r.area()` (which takes `&self` and reads two fields)
// is the canonical "user-defined method" shape we know works.

struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    fn area(&self) -> u32 {
        self.width * self.height
    }
}

fn print_area(rect: &Rectangle) {
    println!("{}", rect.area());
}

fn main() {
    let r = Rectangle { width: 30, height: 50 };
    print_area(&r);
}
