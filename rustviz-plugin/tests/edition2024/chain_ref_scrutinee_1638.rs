#![allow(irrefutable_let_patterns)]
fn chain(check: bool) {
    let mut value = 0;
    let mut reference = &mut value;
    if let link = &mut reference && check {
        **link += 1;
    }
    *reference += 1;
}

fn main() { chain(true); }
