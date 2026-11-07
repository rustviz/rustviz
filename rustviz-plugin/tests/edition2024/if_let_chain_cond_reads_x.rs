// #509: the chain's first part reads `x` (`x.is_some()`) on the line where
// its `let` borrows from `x`. The call runs before either arm, so it's
// drawn on x's split dot, half a row above the `if` row; the `if` arm
// starts on that row with the borrow by `a`. It used to keep the old
// drawing: the arm ran back up to the borrow's dot (a Z), and the
// `x -> a` arrow hopped over the split dot.

fn show(_r: &String) {}

fn main() {
    let x = Some(String::from("a"));
    if x.is_some() && let Some(a) = &x {
        show(a);
    } else {
        println!("none");
    }
}
