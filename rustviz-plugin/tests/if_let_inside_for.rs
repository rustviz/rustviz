// Combination test: an `if let` inside a for-loop body. Each
// iteration matches the loop variable against `Some(_)`; the
// destructure event should fire per iteration. Probes whether
// single-arm if-let inlining (#116) composes with for-loop body
// rendering.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let xs: [Option<String>; 2] = [Some(String::from("a")), None];
    for x in &xs {
        if let Some(inner) = x {
            show(inner);
        }
    }
}
