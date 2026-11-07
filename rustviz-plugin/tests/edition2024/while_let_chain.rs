// `while let … && …` (#280): the binding `t` gets a column, bound from
// `v.pop()` and out of scope at the body's closing brace.

fn take(_k: i32) {}

fn main() {
    let mut v = vec![1, 2];
    let lim = 5;
    while let Some(t) = v.pop() && t < lim {
        take(t);
    }
}
