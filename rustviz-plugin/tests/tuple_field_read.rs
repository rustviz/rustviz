// Reading a plain tuple's field (`t.0`). Before #141 the `.0`
// projection panicked in annotated_source_gen. Since #189 the tuple
// has a column per element and the read is a copy out of `t.0`.

fn main() {
    let t = (1, 2);
    let a = t.0;
}
