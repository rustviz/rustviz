// #382: `if let Some(b) = y` with `y: Option<i32>`. The i32 is copied
// into `b` only when the pattern matches, at the start of the if
// branch's column.
fn take(_k: i32) {}

fn main() {
    let y = Some(2);
    if let Some(b) = y {
        take(b);
    } else {
        take(0);
    }
}
