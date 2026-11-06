// An `FnMut` closure can be called again, so calling it doesn't move
// it: `f` still owns its captured String until the end of `main`.

fn main() {
    let mut s = String::from("hi");
    let mut f = move || s.push('!');
    f();
    f();
}
