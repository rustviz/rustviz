// Deferred initialization of Copy values (#234): `let x;` declares x
// without a value, and `x = 5` is its initialization, not a
// reassignment. `y = x` initializes y by copy; x keeps its value.

fn main() {
    let x;
    x = 5;
    let y;
    y = x;
    println!("{} {}", x, y);
}
