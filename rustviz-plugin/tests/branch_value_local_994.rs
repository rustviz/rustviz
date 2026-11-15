// `let v = if … { x } else { … };` where the branch's value is a variable
// declared in that branch (#994): an `if let` binding, a `let` in the
// block, or a match arm's binding. x's String moves into v when the branch
// ends, so nothing is dropped when x goes out of scope; v then hands it
// to the caller.
#![allow(irrefutable_let_patterns)]

fn if_let_value(o: Option<String>, g: String) -> String {
    let v = if let Some(x) = o {
        x
    } else {
        g
    };
    v
}

// The pattern always matches, so the else is dead code (#987): y's String
// (f's) always ends up in w, and h keeps its own.
fn irrefutable_value(f: String, h: String) -> String {
    let w = if let y = f { y } else { h };
    w
}

fn block_local(c: bool, a: String, b: String) -> String {
    let u = if c {
        let z = a;
        z
    } else {
        b
    };
    u
}

fn arm_binding(p: Option<String>, q: String) -> String {
    let t = match p {
        Some(s) => s,
        None => q,
    };
    t
}

// The branch's k is a new variable that shadows the outer k: m gets the
// inner one's String (a's), and the outer k keeps its own.
fn shadowed(c: bool, a2: String, b2: String) -> String {
    let k = String::from("outer");
    let m = if c {
        let k = a2;
        k
    } else {
        b2
    };
    drop(k);
    m
}

// A Copy value: cnt gets a copy of d in the if branch and 0 in the else,
// so it has a value whichever branch ran.
fn copy_value(e: Option<i32>) -> i32 {
    let cnt = if let Some(d) = e {
        d
    } else {
        0
    };
    cnt
}
