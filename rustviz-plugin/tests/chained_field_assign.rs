// Chained field assignment (`o.inner.x = …`) now resolves
// through `expr_to_rap_name` and emits an event on the nested
// field's column. `register_struct_members` already recursively
// registers nested struct fields like `o.inner.x`, so the
// qualified-name lookup in `resource_of_lhs` finds the RAP.
// (#143)

struct Inner {
    x: i32,
}

struct Outer {
    inner: Inner,
}

fn main() {
    let mut o = Outer { inner: Inner { x: 0 } };
    o.inner.x = 5;
}
