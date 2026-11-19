// #1148: a move closure capturing a `&mut`: the moved r's borrow still ends on r.
// Known bug (KNOWN_INVARIANT_FAILURES in rustviz-lib/tests/common/invariants.rs).
fn main() {
    let mut s = String::from("hello");
    {
        let r = &mut s;
        let mut f = move || r.push('!');
        f();
    }
    println!("{}", s);
}
