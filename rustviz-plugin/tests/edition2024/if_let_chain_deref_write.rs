// #603: `*r = a` writes through `r` before `r.len() > 3` is checked, so
// when that check fails the else branch runs with `n` (what `r` points
// to) already overwritten, and its old String dropped at the write.
// When `y` is `None` it runs with the old String. The write is drawn on
// `r`'s column, like any write through a reference.

fn main() {
    let y = Some(String::from("new"));
    let mut n = String::from("old");
    let r = &mut n;
    if let Some(a) = y && { *r = a; r.len() > 3 } {
        println!("long");
    } else {
        println!("short {}", r);
    }
    println!("{}", n);
}
