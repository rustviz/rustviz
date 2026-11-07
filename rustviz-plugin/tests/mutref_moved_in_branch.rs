// A `&mut` reference moved on some paths only (here into `y`; #674's
// `if b { m } else { k }` returned is the same) owns nothing, so the
// join doesn't say anything is dropped on the other paths.
fn push_some(b: bool, z: &mut String) -> usize {
    if b {
        let y = z;
        y.push('a');
    }
    0
}

fn main() {
    let mut s = String::from("s");
    let n = push_some(true, &mut s);
    println!("{} {}", s, n);
}
