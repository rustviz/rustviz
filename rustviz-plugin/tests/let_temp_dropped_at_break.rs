// #553 with the loop exits #410 draws: a `let`-extended temporary
// (#400) declared in a loop body is dropped where a `break` or
// `continue` leaves that iteration, not "at the end of the block".
// The `_ => {}` path does reach the end of the body, and its drop dot
// keeps that wording.
fn f(xs: &[u32]) -> usize {
    let mut n = 0;
    for x in xs {
        let r = &String::from("a");
        match x {
            0 => break,
            1 => continue,
            _ => {}
        }
        n += r.len();
    }
    n
}

fn main() {
    f(&[2, 1, 0]);
}
