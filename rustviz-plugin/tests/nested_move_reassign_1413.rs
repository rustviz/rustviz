// #1413: a value moved in a nested `if` and given a new value later in
// the same branch holds a value after the outer `if` on every path (the
// join said it "may have no value ... may have had none where they
// start", and the use after it was State::Invalid). The inner join says
// its old value is dropped where it's assigned again. And the reverse: a
// branch that moves it and then only maybe gives it a new value leaves
// it unusable after the outer `if` (its line went on as if held).
fn moved_then_reassigned() {
    let mut s = String::from("hi");
    let c = s.len() > 1;
    if c {
        if c {
            drop(s);
        }
        s = String::from("hey");
    }
    s.push('!');
}

fn moved_then_maybe_reassigned() {
    let mut m = String::from("hi");
    let c = m.len() > 1;
    if c {
        drop(m);
        if c {
            m = String::from("hey");
        }
    }
}

fn in_else_if() {
    let k = String::from("hey");
    let mut p = (String::from("a"), 1);
    if k.len() > 5 {
    } else if k.len() > 1 {
        drop(p);
        if k.len() > 2 {
            p = (String::from("b"), 2);
        }
    }
}

fn main() {
    moved_then_reassigned();
    moved_then_maybe_reassigned();
    in_else_if();
}
