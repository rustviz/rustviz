// #1501: a value `if` / `match` arm that can leave early before its value
// (`if c { if r.is_empty() { return 1; } s } ..`). The exit comes before
// the arm's value moves s, so s still holds its String there and the exit
// drops it.
fn first(c: bool) -> usize {
    let s = String::from("hi");
    let r = &s;
    let t = if c {
        if r.is_empty() { return 1; }
        s
    } else {
        String::new()
    };
    t.len()
}

fn second(c: u8) -> usize {
    let s2 = String::from("hi");
    let r2 = &s2;
    let t2 = match c {
        0 => {
            if r2.is_empty() { return 1; }
            s2
        }
        _ => String::new(),
    };
    t2.len()
}

fn third(c: bool, d: bool) -> usize {
    let s3 = String::from("hi");
    let t3;
    t3 = if c {
        if d { panic!("no"); }
        s3
    } else {
        String::new()
    };
    t3.len()
}

fn fourth(c: bool, o: Option<u8>) -> Option<usize> {
    let s4 = String::from("hi");
    let t4 = if c {
        o?;
        s4
    } else {
        String::new()
    };
    Some(t4.len())
}

fn main() {
    first(true);
    second(0);
    third(true, false);
    fourth(true, None);
}
