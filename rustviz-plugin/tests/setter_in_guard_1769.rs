// A guard's call stores &guard_t in guard_refs, then the guard turns out
// false: the next arm runs with the loan live (rustc rejects
// `guard_t.clear()` there), so its read keeps guard_t shared (#1746).
// The guard's own dot still says the borrow ended, and a segment starts
// with no dot: #1769 (pinned as known in invariants.rs).
pub fn store_false<'a>(v: &mut Vec<&'a String>, s: &'a String) -> bool {
    v.push(s);
    false
}

pub fn setter_in_guard(k: u8) {
    let a = String::from("a");
    let guard_t = String::from("t");
    let mut guard_refs = vec![&a];
    match k {
        0 if store_false(&mut guard_refs, &guard_t) => {}
        _ => {
            println!("{guard_t}");
        }
    }
    println!("{}", guard_refs.len());
}

fn main() {
    setter_in_guard(0);
}
