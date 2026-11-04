// Two structs declared back to back (#192). Each gets its own struct box
// around its own columns; the first box used to stretch over both
// structs and the second struct got no box.
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let a = Point { x: 1, y: 2 };
    let b = Point { x: 3, y: 4 };
    let c = a.x + b.y;
    println!("{}", c);
}
