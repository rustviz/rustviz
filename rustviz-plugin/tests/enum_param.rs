// An enum-typed fn parameter renders as one opaque owner column,
// same as an enum-typed `let`. Before #131's fn-param dispatch the
// param was treated as a struct and got a phantom per-field column
// (`s.0`) built from the first variant's fields.

enum Shape {
    Circle(f64),
    Square(f64),
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.14 * r * r,
        Shape::Square(w) => w * w,
    }
}

fn main() {
    let c = Shape::Circle(1.0);
    let a = area(c);
}
