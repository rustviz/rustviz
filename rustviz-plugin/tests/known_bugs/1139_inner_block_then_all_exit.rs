// #1139: an inner block before an all-exiting if/else: the block's local is never dropped.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn demo(c: bool) {
    {
        let s = String::from("inner");
        println!("{}", s);
    }
    if c { return; } else { return; }
}
fn main() { demo(true); }
