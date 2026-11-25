// #1511: a `for` loop's `&mut` binding moved into a call (no reborrow:
// `drop` / a generic `T` parameter) has nothing left after the move. Its
// borrow ends there; the loop's iterator keeps the lender borrowed until
// the loop ends.
fn take<T>(_t: T) {}

fn dropped(mut v: Vec<String>) {
    for e in v.iter_mut() {
        drop(e);
    }
}

fn taken_then_break(c: bool) {
    let mut w = vec![String::from("a")];
    for f in w.iter_mut() {
        take(f);
        if c { break; }
    }
}

fn head_borrow(mut u: Vec<String>) {
    for g in &mut u {
        take(g);
    }
}

fn literal_head() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    for h in [&mut a, &mut b] {
        take(h);
    }
    println!("{} {}", a, b);
}

fn main() {
    dropped(vec![String::from("x")]);
    taken_then_break(true);
    head_borrow(vec![String::from("y")]);
    literal_head();
}
