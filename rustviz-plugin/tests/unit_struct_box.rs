// Structs with no fields (#348): no member columns, so each box must
// wrap just its own label. Main drew one box with width="-10" shared
// by both (the back-to-back case of #237).
struct Marker;
struct Empty {}

fn main() {
    let m = Marker;
    let e = Empty {};
}
