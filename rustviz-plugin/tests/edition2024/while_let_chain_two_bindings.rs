// `while let … && let …` with two bindings (#584). The loop body is
// drawn inline, so #510's side-by-side layout for an `if let` arm didn't
// apply: the columns were v, y, t, u and `Copy from y to u` hopped over
// `t`'s dot, where `pop`'s value arrives. `u`'s column now comes right
// after `y`'s (v, y, u, t), so the copy runs straight.

fn take(_k: i32) {}

fn main() {
    let mut v = vec![1, 2];
    let y = Some(3);
    while let Some(t) = v.pop() && let Some(u) = y {
        take(t);
        take(u);
    }
}
