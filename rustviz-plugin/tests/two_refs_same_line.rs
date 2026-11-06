// Two references to the same variable made on one line (#233). Each
// takes its own loan: `a`'s ends after `show(a)`, while `b` still
// borrows `x`, so only `b` gives the borrow back (after `show(b)`).
// Before the fix both refs were matched to the same MIR loan and both
// "returned" the borrow on the `show(a)` line, while `b` was still in use.

fn show(_r: &String) {}

fn main() {
    let x = String::from("x");
    let a = &x; let b = &x;
    show(a);
    show(b);
}
