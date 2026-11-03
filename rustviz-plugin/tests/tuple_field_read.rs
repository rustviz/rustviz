// Reading a plain tuple's field (`t.0`). Must render: before #141 the
// `.0` projection panicked in annotated_source_gen. Known gap, not
// pinned as correct: plain tuples get no per-field columns and their
// creation isn't visualized, so the read itself produces no event.

fn main() {
    let t = (1, 2);
    let a = t.0;
}
