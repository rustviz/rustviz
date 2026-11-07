// `if let Some(t) = v.pop() && let Some(u) = y {` (#583): copying the
// i32 out of `y` changes nothing about `y`, so `y` stays one column
// with the copy on it, as for a lone `if let` (#496). `u`'s column comes
// right after `y`'s, so the copy doesn't hop over `t`'s dot.

fn take(_k: i32) {}

fn main() {
    let mut v = vec![1, 2];
    let y = Some(3);
    if let Some(t) = v.pop() && let Some(u) = y {
        take(t);
        take(u);
    } else {
        take(0);
    }
}
