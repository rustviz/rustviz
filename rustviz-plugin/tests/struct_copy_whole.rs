// #174: copying a Copy struct copies its fields; `q.x` / `q.y` hold
// values (no "No resource is dropped" — there was never one to drop).
#[derive(Clone, Copy)]
struct Pt { x: i32, y: i32 }
fn main() {
    let p = Pt { x: 1, y: 2 };
    let q = p;
    println!("{} {}", p.x, q.y);
}
