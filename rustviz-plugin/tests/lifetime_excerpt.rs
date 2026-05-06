// A struct with a lifetime parameter holding a borrow of another
// String. The canonical lifetime-annotation example from the
// tutorial. The plugin tracks `e.p` as a borrow of `s`.

struct Excerpt<'a> {
    p: &'a str,
}

fn main() {
    let s = String::from("hello");
    let e = Excerpt { p: &s };
    println!("{}", e.p);
}
