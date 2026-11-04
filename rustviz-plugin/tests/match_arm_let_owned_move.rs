// #425: an arm-local `let` next to an outer variable moved in another arm.
// The merge dot may say `s` was moved, never `t` (declared in an arm).
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let n = 2;
    match n {
        0 => {
            take(s);
        }
        _ => {
            let t = String::from("x");
            println!("{}", t);
        }
    }
}
