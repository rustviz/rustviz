// #1122: a `&mut` argument whose call returns a `&`: the lender's column vanishes (State::Invalid).
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn downgrade(s: &mut String) -> &str { s.as_str() }
fn main() {
    let mut s = String::from("hello");
    let r = downgrade(&mut s);
    let _spacer = 1;
    println!("{}", r);
}
