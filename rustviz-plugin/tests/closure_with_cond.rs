// Combination test: a closure whose body is an if/else over a
// captured variable. Rust's closure inference picks the captured
// variable's borrow/move kind by what the body does on each
// path; we want the visualization not to panic on the conditional
// inside a closure body.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let cond = true; // rustviz: skip
    let f = || {
        if cond {
            show(&s);
        } else {
            show(&s);
        }
    };
    f();
}
