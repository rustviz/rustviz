// The backedge can carry a value that the first entry lacks.
pub fn zero_or_more(test: impl Fn() -> bool) {
    let mut value;
    while test() {
        value = String::from("iteration");
    }
    std::hint::black_box(());
}

// The pattern can fail before the body. Testing the pattern does not
// move the payload on that path, even though success moves it.
pub fn refutable(mut item: Option<String>, stop: impl Fn() -> bool) {
    while let Some(payload) = item {
        if stop() {
            drop(payload);
            break;
        }
        item = Some(payload);
    }
    std::hint::black_box(());
}

// Only the labeled break carries a value out of the outer loop. The
// inner break and the outer continue are different CFG destinations.
pub fn nested(stop: impl Fn() -> bool, finish: impl Fn() -> bool) -> String {
    let mut carried = String::from("first");
    let result = 'outer: loop {
        loop {
            if stop() {
                break;
            }
            if finish() {
                break 'outer carried;
            }
            continue 'outer;
        }
        carried = String::from("later");
    };
    result
}

// The loop tail moves a value on the way to the next iteration. All
// normal exits instead follow a write.
pub fn replaced(done: impl Fn() -> bool) -> String {
    let mut value = String::from("initial");
    loop {
        value = String::from("replacement");
        if done() {
            break;
        }
        drop(value);
    }
    value
}

pub fn exhausted(items: Vec<String>) {
    let mut last;
    for item in items {
        last = item;
    }
    std::hint::black_box(());
}

// This loop can retry, but the only write is on the exit edge. It
// can't overwrite a value assigned by an earlier iteration.
pub fn assign_once(ready: impl Fn() -> bool) -> String {
    let mut once;
    loop {
        if !ready() {
            continue;
        }
        once = String::from("assigned once");
        break;
    }
    once
}
