// A borrow of a temporary whose borrow also ends on the same line
// (the reference is never used again). The borrow's start used to be
// dropped because it isn't in the renderer's line map (#204).
struct Point {
    x: i32,
}

fn main() {
    let r = &Point { x: 4 };
    let m = &mut Point { x: 5 };
    let add_one = |x: i32| x + 1;
    let y = add_one(5);
    println!("{y}");
}
