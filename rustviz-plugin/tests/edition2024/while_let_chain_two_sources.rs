// `while let Some(b) = y && let Some(c) = z {` (#584): both bindings
// are copied out of variables on the same line. Each binding's column
// comes right after its source's (y, b, z, c) and both arrows run
// straight along the `while let` row, with no row added below it.

fn take(_k: i32) {}

fn main() {
    let y = Some(2);
    let z = Some(3);
    while let Some(b) = y && let Some(c) = z {
        take(b);
        take(c);
        break;
    }
}
