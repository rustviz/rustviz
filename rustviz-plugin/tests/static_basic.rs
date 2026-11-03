// `static` items with `'static` lifetime. The static itself isn't
// owned by any function-local binding; uses copy or borrow into
// local bindings depending on type. Verifies the plugin doesn't
// trip on the special `'static` lifetime when borrowing from the
// static.

static GREETING: &str = "hello";

fn show(_s: &str) {} // rustviz: hide

fn main() {
    let s = GREETING;
    show(s);
}
