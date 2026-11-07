// The first part of a let-chain always runs, so its events are drawn
// before the split, like a lone `if let`'s scrutinee (#198): `s` is
// moved into `wrap` whether or not `w.len() > 1` then holds. The
// bindings (`w`) are drawn in the if branch.

fn wrap(s: String) -> Option<String> { Some(s) }
fn show(_r: &String) {}

fn main() {
    let s = String::from("a");
    if let Some(w) = wrap(s) && w.len() > 1 {
        show(&w);
    } else {
        println!("short");
    }
}
