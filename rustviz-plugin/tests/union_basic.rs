// Basic `union` use. Pre-fix the plugin panicked at the
// AdtKind::Union arm with "union not implemented yet". We now
// render unions as opaque single-owner columns (same shape as
// enums) — the union has a column, but per-field timelines
// aren't registered because there's no statically-known
// discriminant.

union Pair {
    _a: i32,
    _b: f32,
}

fn show(_p: Pair) {} // rustviz: hide

fn main() {
    let u = Pair { _a: 42 };
    show(u);
}
