// #579: `f` moves `s` out of its captures when called, so it's FnOnce
// and calling it moves it. The call runs only when `y` matched, and the
// else branch also runs when `f()` returns false: there `f` may have
// been moved, and its captured String is dropped at the end only if it
// wasn't.

fn main() {
    let s = String::from("a");
    let y = Some(1);
    let f = move || { let t = s; t.len() > 3 };
    if let Some(_n) = y && f() {
        println!("long");
    } else {
        println!("short or none");
    }
}
