// Each use of a constant produces a value. Rust promotes the borrowed
// i32 and Pt here. Borrowing EMPTY creates a fresh Vec temporary, which
// the let keeps alive until the end of this block.
// The borrow held by v ends after its final use. Its Vec drops at block end.

struct Pt { x: i32, y: i32 }
const ANSWER: i32 = 42;
const ORIGIN: Pt = Pt { x: 0, y: 0 };
const EMPTY: Vec<i32> = Vec::new();

fn main() {
    let p = &ANSWER;
    let q = &ORIGIN;
    let v = &EMPTY;
    println!("{} {} {} {}", p, q.x, q.y, v.len());
}
