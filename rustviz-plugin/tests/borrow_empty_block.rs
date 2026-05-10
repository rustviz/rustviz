// `&{}` — borrowing the unit value of an empty block. Used to
// panic in fetch_mutability before #146; now propagates None
// from the empty-block arm so the outer AddrOf falls back to
// its declared mutability. Pins the no-crash baseline.

fn main() {
    let _r = &{};
}
