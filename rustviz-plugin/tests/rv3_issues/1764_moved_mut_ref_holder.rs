struct P { a: String, b: String }
fn moved_holder(mut p: P) {
    let r = &mut p;
    let t = r;
    t.b.push('!');
    t.b.push('?');
}
