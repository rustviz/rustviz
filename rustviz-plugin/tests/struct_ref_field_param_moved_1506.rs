// #1506: a struct whose reference field is a copy of a reference
// parameter, moved away: the field goes with the struct, so its borrow
// ends at the move. The caller's String stays borrowed by the parameter
// itself, which has no lender column here. Also a struct moved into a
// value that holds its borrow from then on (`let c = Some(w);`).
struct Holder<'a> {
    r: &'a String,
    n: i32,
}

impl<'a> Holder<'a> {
    fn consume(self) -> i32 {
        self.n
    }
}

fn take(_h: Holder) {}
fn take_gen<T>(_t: T) {}

fn by_drop(r: &String) {
    let h = Holder { r: r, n: 1 };
    drop(h);
    println!("done");
}

fn by_take(r: &String) {
    let k = Holder { r: r, n: 1 };
    take(k);
    println!("done");
}

fn by_gen(r: &String) {
    let g = Holder { r: r, n: 1 };
    take_gen(g);
    println!("done");
}

fn by_method(r: &String) {
    let m = Holder { r: r, n: 1 };
    let n = m.consume();
    println!("{}", n);
}

fn into_some(r: &String) {
    let q = Holder { r: r, n: 1 };
    let o = Some(q);
    println!("{}", o.is_some());
}

fn local_into_some() {
    let s = String::from("s");
    let w = Holder { r: &s, n: 2 };
    let c = Some(w);
    println!("{}", c.map(|w| w.n).unwrap_or(0));
}

// Moved in one arm only: on the other path b isn't used again (it can't
// be, after a maybe-move), so b.r's borrow ends at the move there too.
fn in_branch(r: &String, c: bool) {
    let b = Holder { r: r, n: 1 };
    if c {
        drop(b);
    }
    println!("done");
}

fn main() {
    let s = String::from("s");
    by_drop(&s);
    by_take(&s);
    by_gen(&s);
    by_method(&s);
    into_some(&s);
    local_into_some();
    in_branch(&s, true);
}
