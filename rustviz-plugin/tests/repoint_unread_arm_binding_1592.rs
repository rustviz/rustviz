// A reference with no loan of its own (copied out of a value, or a copy of
// another reference), dead before it is re-pointed: its old borrow ends at
// its last use, or where it was bound, not at the write (#1592).

// The issue's program: x is never read before `x = &s2;`.
fn arm_binding() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    match v.first() {
        Some(mut x) => {
            x = &s2;
        }
        None => {}
    }
}

// Read first, then dead for a line before the write.
fn arm_binding_read() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    match v.first() {
        Some(mut x) => {
            println!("{}", x);
            let n = 1;
            x = &s2;
        }
        None => {}
    }
}

// Read by the arm's guard only.
fn arm_binding_guard() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    match v.first() {
        Some(mut x) if x.len() > 1 => {
            x = &s2;
        }
        _ => {}
    }
}

// Matching an Option held in a variable.
fn arm_binding_held() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    let o = v.first();
    match o {
        Some(mut x) => {
            x = &s2;
        }
        None => {}
    }
}

// A `while let` binding.
fn while_let_binding() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    let mut it = v.iter();
    while let Some(mut x) = it.next() {
        x = &s2;
    }
}

// A copy of another reference.
fn copied_ref() {
    let a = String::from("a");
    let b = String::from("b");
    let x = &a;
    let mut r = x;
    let n = 1;
    r = &b;
    println!("{}", r);
}

// Already right: an `if let` binding.
fn if_let_binding() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    if let Some(mut x) = v.first() {
        x = &s2;
    }
}

fn main() {
    arm_binding();
    arm_binding_read();
    arm_binding_guard();
    arm_binding_held();
    while_let_binding();
    copied_ref();
    if_let_binding();
}
