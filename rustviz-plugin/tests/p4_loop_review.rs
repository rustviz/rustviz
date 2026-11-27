// Both the condition's false edge and the explicit break reach the join.
pub fn condition_and_break(mut x: String, go: impl Fn() -> bool, stop: impl Fn() -> bool) {
    while {
        drop(x);
        go()
    } {
        if stop() {
            break;
        }
        x = String::new();
    }
}

pub fn unassigned_at_entry(flag: bool, go: impl Fn() -> bool) {
    let mut n: i32;
    if flag {
        n = 1;
    }
    while go() {
        n = 2;
    }
}

pub fn initialized_before_loop(flag: bool, tick: impl Fn() -> bool) {
    let mut x;
    if flag {
        x = String::new();
    }
    while tick() {}
    x = String::new();
    std::hint::black_box(&x);
}

// Borrowck checks a path that skips the assignment. Every real exit writes x.
pub fn unconditional_assignment(input: String) {
    let x;
    loop {
        if let s = input {
            x = s;
        }
        break;
    }
}

pub fn drops_replacement(mut s: String, stop: impl Fn() -> bool) {
    drop(s);
    loop {
        if stop() {
            break;
        }
        s = String::from("intermediate");
        drop(s);
        s = String::from("surviving replacement");
    }
}

pub fn return_from_operand(stop: bool, x: String) -> String {
    let n = loop {
        break if stop { return x; } else { 123 };
    };
    format!("{n}: {x}")
}

pub fn drop_in_tuple(x: String) -> ((), i32) {
    let result = loop {
        break (drop(x), 1);
    };
    result
}

pub fn drop_in_block(s: String) -> i32 {
    let result = loop {
        break {
            drop(s);
            1
        };
    };
    result
}

pub fn imaginary_backedge(input: String) {
    loop {
        if let s = input {
            drop(s);
            break;
        }
    }
}

pub fn constant_condition() {
    while false {
        std::hint::black_box(());
    }
}

struct Guard<'a>(&'a mut String);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.push('!');
    }
}

pub fn cleanup_before_continuation(enter: bool, skip: bool, early: bool, mut text: String) {
    if enter {
        loop {
            if skip {
                break;
            }
            let guard = Guard(&mut text);
            let pair = (String::new(), String::new());
            drop(pair.0);
            if early {
                break;
            }
            std::hint::black_box(());
            break;
        }
    }
    text.push('?');
}

// A move used to construct the break result is a transfer out of the loop.
pub fn carried_in_tuple(value: String, discard: bool) -> Option<(String, i32)> {
    let result = loop {
        if discard {
            drop(value);
            break None;
        }
        break Some((value, 1));
    };
    result
}
