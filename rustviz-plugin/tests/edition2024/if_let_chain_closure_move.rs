// #281: the condition may move the closure `f` into `run` before `run`
// returns false, so whether `f`'s captured String is dropped at the end
// depends on whether something above moved it.

fn run<F: FnOnce() -> bool>(f: F) -> bool { f() }

fn main() {
    let s = String::from("a");
    let y = Some(1);
    let f = move || s.len() > 0;
    if let Some(_n) = y && run(f) {
        println!("yes");
    } else {
        println!("no");
    }
}
