// #395: `t.0` and `t.1.1` are moved out only when the first arm is
// taken. If the `_` arm runs, they still own their Strings, and Rust
// drops them at the end of `main` (a runtime drop flag decides). So
// `t`'s scope end can't say they "were moved out earlier".

fn main() {
    let t = (String::from("a"), (1, String::from("b")));
    match t {
        (first, (n, second)) if n > 0 => println!("{} {}", first, second),
        _ => {}
    }
}
