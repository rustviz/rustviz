fn maybe_moved() {
    let c = std::env::args().len() > 1;
    let mut s = String::from("old");
    if c { drop(s); }
    if c { return; }
    s = String::from("new");
}

fn maybe_assigned() {
    let c = std::env::args().len() > 1;
    let mut s: String;
    if c { s = String::from("first"); }
    if c { return; }
    s = String::from("last");
}

fn let_else(c: bool, next: Option<()>) {
    let mut x = String::from("old");
    if c { drop(x); }
    let Some(()) = next else { return };
    x = String::from("new");
}

fn main() {}
