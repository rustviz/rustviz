// `&CONST` borrows a fresh copy of the constant. Rust stores that copy
// for the whole program only when the value has no destructor and no
// interior mutability: `p` (an i32) and `q` (a struct of integers)
// get the constant wording; `v` (a Vec, which has a destructor) does
// not, since its copy is an ordinary temporary.
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
