// #1571: a shared reference holding a mutable loan, declared in an arm
// and never used (`let r3 = downgrade(&mut s2);`, `let r = r0;` with
// `r0: &String = &mut s`): its borrow ends as a mutable one, as the dot
// above it says ("the borrow stays mutable").
fn downgrade(s: &mut String) -> &String {
    s
}

fn in_arm() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = downgrade(&mut s2);
        let m2 = &mut s2;
    } else {
        m.push('a');
    }
}

fn copy_in_loop(v: Vec<i32>) {
    let mut s = String::from("rust");
    let r0: &String = &mut s;
    if v.is_empty() {
        for e in v {
            let r = r0;
        }
    }
}

fn main() {
    in_arm();
    copy_in_loop(vec![1]);
}
