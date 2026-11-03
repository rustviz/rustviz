// `const` items. Compile-time values; copying them into a binding
// shouldn't leave a phantom "const item is moved" event on a
// timeline for the const itself, and the receiving binding should
// behave like a normal Copy.

const N: i32 = 5;

fn main() {
    let x = N;
    let y = x;
    let _ = y; // rustviz: skip
}
