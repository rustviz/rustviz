// Tuple destructure assignment `(a, b) = (b, a)` (Rust ≥1.59).
// rustc desugars to `{ let (lhs, lhs) = (b, a); a = lhs; b = lhs; }`,
// so the per-element assignments hit the existing Assign + Path
// path. Before #151 the synthetic `lhs` local got registered as its
// own RAP and surfaced as a phantom column on the diagram. Now the
// desugar's pattern is detected and the local skipped — only `a`
// and `b` show timelines, each picking up the swap event.

fn main() {
    let mut a = 1;
    let mut b = 2;
    (a, b) = (b, a);
}
