// #1136: `?` after an FnOnce call draws a drop of the closure already consumed.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn bar() -> Result<(), ()> { Ok(()) }
fn demo() -> Result<(), ()> {
    let s = String::from("a");
    let f = move || drop(s);
    f();
    bar()?;
    Ok(())
}
fn main() { let _ = demo(); }
