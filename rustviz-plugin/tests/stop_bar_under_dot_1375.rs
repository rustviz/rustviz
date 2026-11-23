fn take(s: String) -> usize { s.len() }
fn call_returned(c: bool) -> usize {
    let q = String::from("q");
    if c {
        return 1;
    } else {
        return take(q);
    }
}
fn operator_returned(c: bool) -> String {
    let q = String::from("q");
    if c {
        return q;
    } else {
        return q + "!";
    }
}
fn param_returned(q: String, c: bool) -> usize {
    if c {
        return 1;
    } else {
        return take(q);
    }
}
fn breaks_after_push() -> String {
    let mut s = String::new();
    let mut n = 0;
    loop {
        n += 1;
        if n < 3 {
            s.push('a');
        } else {
            s.push('b'); break;
        }
    }
    s
}
fn panics_with(c: bool) {
    let w = String::from("w");
    if c {
        println!("{}", w);
    } else {
        panic!("{}", w);
    }
}
fn main() {
    call_returned(true);
    operator_returned(true);
    param_returned(String::new(), true);
    breaks_after_push();
    panics_with(true);
}
