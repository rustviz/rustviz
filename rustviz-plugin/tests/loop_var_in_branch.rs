// A loop inside one branch of an if/else or match (#261). The loop's
// variable only exists inside the loop, so it isn't live across the
// conditional; before the fix it got a branched column ending in
// "x was moved in at least one branch above; …" at the merge point.

fn take(_k: i32) {}

fn main() {
    let b = true;
    let mut v = vec![1];
    if b {
        while let Some(x) = v.pop() {
            take(x);
        }
    } else {
        v.push(2);
    }
    let o = Some(3);
    match o {
        Some(n) => {
            for i in 0..n {
                take(i);
            }
        }
        None => {
            take(0);
        }
    }
}
