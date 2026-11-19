// #1133: a let-else whose else block moves s is drawn inline: s used and dropped after its move.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn demo(opt: Option<i32>) {
    let s = String::from("hello");
    let Some(x) = opt else {
        drop(s);
        return;
    };
    println!("{} {}", s, x);
}
fn main() { demo(Some(1)); }
