// #1421: a struct holding a reference, declared in a nested branch (an
// `else if` arm, an `if` inside an `else`), copying another's reference.
// Its reference field is initialized with the copy, then its borrow ends,
// in that order, and it goes out of scope with the struct (it used to say
// its borrow ended before it was initialized, and had no scope end).
struct Holder<'a> {
    r: &'a String,
}

fn else_if_one_row() {
    let s = String::from("a");
    let h = Holder { r: &s };
    if s.is_empty() {
    } else if s.len() > 1 {
        let h2 = Holder { r: h.r };
    } else {
    }
}

fn else_if_used() {
    let t = String::from("a");
    let g = Holder { r: &t };
    if t.is_empty() {
    } else if t.len() > 1 {
        let g2 = Holder { r: g.r };
        println!("{}", g2.r);
    } else {
    }
}

fn nested_if() {
    let u = String::from("a");
    let k = Holder { r: &u };
    if u.is_empty() {
    } else {
        if u.len() > 1 {
            let k2 = Holder { r: k.r };
        }
    }
}

fn then_arm() {
    let v = String::from("a");
    let j = Holder { r: &v };
    if v.is_empty() {
        let j2 = Holder { r: j.r };
    }
}

fn main() {
    else_if_one_row();
    else_if_used();
    nested_if();
    then_arm();
}
