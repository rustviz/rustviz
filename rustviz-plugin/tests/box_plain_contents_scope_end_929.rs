// #929: a Box whose contents need no drop (P holds only an i32), moved
// out in some branches only. At b's scope end nothing of the contents is
// dropped on any path, so no drop flag matters; only the Box's heap
// allocation is freed, on every path. The scope end said "Its resource
// is dropped here only if no branch above moved it (Rust tracks this
// with a runtime drop flag)".

struct P { a: i32 }
fn take(_p: P) {}

fn main() {
    let b = Box::new(P { a: 1 });
    let k = 3;
    match k {
        4 => take(*b),
        _ => {}
    }

    // The same with an `if` without `else`.
    let c = Box::new(P { a: 2 });
    if k == 5 {
        take(*c);
    }
}
