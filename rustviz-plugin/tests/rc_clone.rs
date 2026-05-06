// `Rc::new` + `Rc::clone(&r)` — issue #76 case 2.
// `Rc::clone(&r)` is a regular function call: `&r` is a static borrow
// into `Rc::clone`, the return value is moved into `r2`. The
// shared-ownership semantics aren't currently visualized for any
// type — the immediate goal here is "don't crash and don't expose
// internals like `r.ptr.pointer`."

use std::rc::Rc;

fn main() {
    let r = Rc::new(String::from("hi"));
    let r2 = Rc::clone(&r);
    println!("{} {}", r, r2);
}
