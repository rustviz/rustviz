// Single-arm `if let Some(x) = opt { … }` over an Option<String>
// (non-Copy). With no else clause, the visualization inlines the
// destructure and body events onto the parent timeline — no
// Branch event, no zigzag column, no merge dot. The destructure
// emits a Move from opt to x; the body emits a borrow on x; x
// goes out of scope at the closing brace.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let opt: Option<String> = Some(String::from("x"));
    if let Some(x) = opt {
        show(&x);
    }
}
