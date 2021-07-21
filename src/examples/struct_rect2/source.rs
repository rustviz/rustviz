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
    println!(
        "The area of the rectangle is {} square pixels.",
       	rect.area() // dot even though it's actually a reference
    );
}

fn main() {
    let r = Rectangle {
        width: 30,
        height: 50,
    };

    print_area(&r);
}