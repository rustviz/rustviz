// Basic `union` use. Pre-fix the plugin panicked at the
// AdtKind::Union arm with "union not implemented yet", and a field
// write (`u._a = …`) panicked looking up a `u._a` RAP. We render
// unions as opaque single-owner columns (same shape as enums): the
// union has a column, but per-field timelines aren't registered
// because there's no statically-known active field. Field writes,
// reads and borrows are all attributed to `u` itself.

union Pair {
    _a: i32,
    _b: f32,
}

fn show(_p: Pair) {} // rustviz: hide

fn main() {
    let mut u = Pair { _a: 42 };
    u._b = 1.5;
    let y = 7;
    u._a = y;
    let x = unsafe { u._a };
    let r = unsafe { &u._b };
    show(u);
}
