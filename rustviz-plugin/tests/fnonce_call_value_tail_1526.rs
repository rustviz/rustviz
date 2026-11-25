// #1526: an FnOnce closure called as the value of a labeled block or of a
// value `if` / `match` arm. The call consumes the closure on that path
// only: after the block or the join, it may have been moved, and what it
// captured is dropped at its scope end only if it wasn't called.
fn first() {
    let s = String::from("x");
    let s2 = String::from("rust");
    let c = move || s;
    let s4 = 'blk: {
        if s2.len() > 1 { break 'blk s2; }
        c()
    };
}

fn second() {
    let s5 = String::from("hi");
    let c2 = move || s5;
    let n = 3;
    let t = if n > 1 {
        String::new()
    } else {
        c2()
    };
}

fn third() {
    let s6 = String::from("hi");
    let c3 = move || s6;
    let n = 3;
    let t3 = match n {
        1 => String::new(),
        _ => c3(),
    };
}

fn fourth() {
    let s7 = String::from("hi");
    let c4 = move || s7;
    let n = 3;
    let t4 = if n > 1 {
        c4()
    } else {
        String::new()
    };
}

fn fifth() {
    let s8 = String::from("x");
    let s9 = String::from("rust");
    let c5 = move || s8;
    let t5 = loop {
        if s9.len() > 1 { break s9; }
        break c5();
    };
}

fn main() {
    first();
    second();
    third();
    fourth();
    fifth();
}
