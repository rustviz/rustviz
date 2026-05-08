// Inline ref-arg call inside `println!("{}", …)` — issue #74.
// The user's `r.get()` is a `&self` method call (ref-arg call site).
// Pre-fix, modern `println!` expanded to a synthetic block whose
// outer span is from-expansion, so the visitor's macro-skip dropped
// the entire arg subtree and the `get reads from r` arrow never
// fired. After the fix, the visitor descends through synthetic
// scaffolding until it reaches user-spanned subexpressions, so the
// `MethodCall` arm runs and emits the call-site arrow as if the
// method had been written at statement position.

struct R { n: i32 }

impl R {
    fn get(&self) -> i32 { self.n }
}

fn main() {
    let r = R { n: 5 };
    println!("{}", r.get());
}
