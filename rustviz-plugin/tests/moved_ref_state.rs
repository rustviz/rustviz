// #292: after `let q2 = q;` moves the `&mut` out of `q`, q's timeline
// says the reference was moved out, not that q lost "ownership" (a
// reference never had any). `q = &mut s;` gives q a new reference.
fn main() {
    let mut t = String::from("t");
    let mut s = String::from("s");
    let mut q = &mut t;
    let q2 = q;
    q2.push('.');
    q = &mut s;
    q.push(',');
    println!("{} {}", s, t);
}
