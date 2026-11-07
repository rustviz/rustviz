// `if let Some(b) = y && let Some(a) = &x {` with no `else` (#583): the
// body only reads, so it's drawn inline. Both bindings are made on the
// `if let` line, so each binding's column comes right after its
// source's (x, a, y, b) and both arrows run straight along that line,
// with no row added below it.

fn show(_r: &String) {}
fn take(_k: i32) {}

fn main() {
    let x = Some(String::from("a"));
    let y = Some(2);
    if let Some(b) = y && let Some(a) = &x {
        show(a);
        take(b);
    }
}
