// `move` closure that captures two distinct upvars. Each capture
// becomes its own Move arrow into the closure binding.

fn main() {
    let s = String::from("hi");
    let t = String::from("bye");
    let f = move || println!("{} {}", s, t);
    f();
}
