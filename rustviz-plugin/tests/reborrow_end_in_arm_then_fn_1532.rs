fn run(m: &mut String, b: bool) {
    let m2 = &mut *m;
    if b {
    } else {
        m2.push('a');
    }
}
fn main() {}
