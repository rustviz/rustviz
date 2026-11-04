// A `move` closure owns what it captured, so calling it uses its own
// `s`, not the variable `s` (whose column ends at the capture). The
// calls are marked on `f`'s column only (#133).

fn main() {
    let s = String::from("hi");
    let f = move || println!("{}", s);
    f();
    f();
}
