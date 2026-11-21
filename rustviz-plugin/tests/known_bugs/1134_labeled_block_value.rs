// #1134: `break 'blk s` out of a labeled block is ignored: t's move is drawn unconditional, then t is dropped.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn demo(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let x = 'blk: {
        if c {
            break 'blk s;
        }
        t
    };
    println!("{}", x);
}
fn main() { demo(true); }
