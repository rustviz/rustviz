// Index assignment (`v[i] = …`). The plugin renders Vec /
// array as an opaque single-owner column, so the assignment
// attributes its event to the receiver's column rather than
// fabricating a per-element timeline. (#144)

fn main() {
    let mut v = vec![1, 2, 3];
    v[0] = 9;
    println!("{}", v[0]);
}
