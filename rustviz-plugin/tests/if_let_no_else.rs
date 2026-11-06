// Single-arm `if let Some(x) = opt { … }` over an Option<String>
// (non-Copy). The destructure emits a Move from opt to x, the body
// emits a borrow on x, and x goes out of scope at the closing brace.
// The pattern moves the String out of `opt` only when it matches, so
// `opt` is maybe-moved afterwards: its column is drawn split at the
// `if let` with an implicit empty `else`, and the merge says opt was
// moved in one branch (#257, #517).

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let opt: Option<String> = Some(String::from("x"));
    if let Some(x) = opt {
        show(&x);
    }
}
