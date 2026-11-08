// A String lent inside a `loop`'s if/else where every branch `break`s
// (#759): the borrow ends in the branch, the String is never moved, so
// `a` is dropped at the end of `main`. The old plugin ended `a`'s
// column when the branches had all diverged and said "No resource is
// dropped".

fn main() {
    let a = String::from("a");
    let ra = &a;
    let c = a.len() > 0;
    loop {
        if c { break; } else { let x = ra; break; }
    }
}
