// RustViz 3 fact layer: the cases the review of #1851 found, pinned by
// `facts_pin_review_1851` in rustviz-lib/tests/corpus.rs.
#![feature(try_blocks)]

struct Src;
impl Src {
    fn get(&self) -> Option<u32> {
        None
    }
}
impl Drop for Src {
    fn drop(&mut self) {}
}

fn checked_add(a: String, b: String) {
    let n = a.len() + b.len();
    let _m = n;
}

fn letelse_return() {
    let Some(x) = Src.get() else {
        return;
    };
    let _y = x;
}

fn two_returns(o: Option<String>, a: bool) {
    let s = String::new();
    let Some(x) = o else { if a { return; } return; };
    let n = 1;
}

fn try_tail(o: Option<()>) -> Option<()> {
    let d = String::new();
    try {
        o?;
    }
}

fn capture_match() {
    let z = 1;
    let y = String::new();
    let c = || {
        match z { _ => () }
        y.len()
    };
    c();
}

fn iter_named(iter: Vec<String>) {
    for s in iter {
        let _n = s.len();
    }
}

fn two_returns_in_loop(v: Vec<String>, a: bool) -> u8 {
    for s in v {
        if a { return 1; }
        if s.is_empty() { return 2; }
    }
    0
}

fn break_in_loop(v: Vec<String>) {
    for s in v {
        if s.is_empty() { break; }
    }
    let n = 1;
}
