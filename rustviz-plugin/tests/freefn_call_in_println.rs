// Free-fn ref-arg call inside `println!("{}", …)` — companion to
// `method_call_in_println.rs`. `get(&r)` is the same shape as the
// method-call case from issue #74 (a ref-arg call), only via a free
// function. Pre-fix the visitor's macro-skip dropped this too; the
// fix's descend-through-expansion walks the synthetic scaffolding
// until it reaches the user's `get(&r)` Call and the regular Call
// arm emits the `get reads from r` arrow.

struct R { n: i32 }

fn get(r: &R) -> i32 { r.n }

fn main() {
    let r = R { n: 5 };
    println!("{}", get(&r));
}
