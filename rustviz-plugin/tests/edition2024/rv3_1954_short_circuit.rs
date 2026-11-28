fn chain(flag: bool) -> u32 {
    let calls = std::cell::Cell::new(0);
    let f = || { calls.set(calls.get() + 1); false };
    if let ref keep = f && flag && f() {
        std::hint::black_box(keep);
    } else {
        std::hint::black_box(0);
    }
    calls.get()
}

fn main() {
    assert_eq!(chain(false), 0);
    assert_eq!(chain(true), 1);
}
