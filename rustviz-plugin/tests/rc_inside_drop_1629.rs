use std::rc::{Rc, Weak};

struct Two { a: Rc<String>, b: Rc<String> }

// o holds the only Rc: dropping it frees the String.
fn last_rc_in_option() {
    let o = Some(Rc::new(String::from("x")));
    drop(o);
}

// An empty Vec holds no Rc and has no buffer.
fn empty_vec_of_rc() {
    let v: Vec<Rc<String>> = Vec::new();
    drop(v);
}

fn no_weak() {
    let w: Option<Weak<String>> = None;
    drop(w);
}

fn rc_fields() {
    let t = Two { a: Rc::new(String::new()), b: Rc::new(String::new()) };
    drop(t);
}
