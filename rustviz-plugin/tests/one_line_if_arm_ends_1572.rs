// #1572: a conditional all on one line. The end of a borrow whose last
// use is in one arm was left on the line beside the arms, so it was drawn
// on the fork or join dot: m2's end on the join, after s2 was read in the
// other arm while drawn mutably borrowed (`State::Invalid`); r2's new
// loan's end on the dot above the fork, before the `&mut` existed. Each
// now ends in its arm, as in the multi-line form.
fn read_other_arm(c: bool) {
    let mut s2 = String::from("a");
    let m2 = &mut s2;
    let n = if c { s2.len() } else { m2.len() };
}

fn replace_other_arm() {
    let s = String::from("hi");
    let mut s3 = String::from("rust");
    let m3 = &mut s3;
    let s4 = if s.len() > 1 { s3.to_string() } else { std::mem::replace(m3, String::new()) };
}

fn take(n: usize) {}

fn call_arg(c: bool) {
    let mut s5 = String::from("a");
    let m5 = &mut s5;
    take(if c { s5.len() } else { m5.len() });
}

fn repoint_in_else(t: String) {
    let mut s6 = String::from("rust");
    let mut r6 = &t;
    if s6.len() > 1 { } else { r6 = &mut s6; }
}
