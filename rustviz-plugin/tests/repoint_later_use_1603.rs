// A re-point that borrows the same lender again (Codex review of #1603):
// `&s` is taken before the write ends the old loan, so s is borrowed by the
// new loan from then on, never free in between.

struct Holder<'a> {
    r: &'a String,
}

fn same_lender_again(s: String) {
    let mut h = Holder { r: &s };
    let n = 1;
    h.r = &s;
}

fn main() {}
