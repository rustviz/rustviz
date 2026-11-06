// Creating a tuple and copying an element out of it (#189). The
// String goes into `t.0`; reading `t.1` copies an i32 and leaves `t`
// whole, so `t` still drops its String at the end.

fn main() {
    let t = (String::from("a"), 1);
    let n = t.1;
    println!("{:?} {}", t, n);
}
