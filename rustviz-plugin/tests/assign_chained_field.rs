// Chained field LHS on the LHS of an assignment is currently
// unsupported (issue #143) — the plugin used to panic on this
// shape but now gracefully skips emitting an event for the
// assignment so the rest of the program still renders. This
// fixture pins the no-crash behavior; once #143 is implemented,
// the chained-field assignment should produce a real event.

struct Inner {
    x: i32,
}

struct Outer {
    inner: Inner,
}

fn main() {
    let mut o = Outer { inner: Inner { x: 0 } };
    o.inner.x = 5;
    let _ = o.inner.x; // rustviz: skip
}
