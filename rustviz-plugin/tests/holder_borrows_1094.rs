// Values that hold references without being references ("holders"),
// #1094-#1097. Each claim was checked against rustc (E0505 on a
// conflicting `drop`).

fn keepv<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

// #1094: w holds copies of the references stored in v (they point to
// s), not a borrow of v: `drop(v)` compiles while w is in use, and
// `drop(s)` there doesn't.
fn main() {
    let s = String::from("s");
    let mut v = Vec::new();
    keepv(&mut v, &s);
    let w: Vec<&String> = v.iter().copied().collect();
    drop(v);
    println!("{}", w.len());
}

// #1095: a holder reassigned from a literal borrows what the literal
// holds: a until o's last use, c until arr's (rustc rejects `drop(a)` /
// `drop(c)` before them); b and d are free after the reassignment.
fn reassigned() {
    let a = String::from("a");
    let b = String::from("b");
    let mut o = vec![&b];
    o = vec![&a];
    println!("{}", o[0]);
    let c = String::from("c");
    let d = String::from("d");
    let mut arr = [&d];
    arr = [&c];
    println!("{}", arr[0]);
}

struct Shelf<'a> {
    items: Vec<&'a str>,
}

// #1096: a struct field filled later stores e's borrow, which first,
// copied out of it, keeps (rustc rejects `drop(e)` before the println).
fn filled_later() {
    let e = String::from("e");
    let mut shelf = Shelf { items: Vec::new() };
    shelf.items.push(e.as_str());
    let first = shelf.items[0];
    println!("{}", first);
}

// #1097: overwriting the whole struct ends s2.items's borrow of h and
// drops the old value before s2.items borrows g (`drop(h)` after the
// overwrite compiles; `drop(g)` before the println doesn't).
fn overwritten() {
    let g = String::from("g");
    let h = String::from("h");
    let mut s2 = Shelf { items: vec![h.as_str()] };
    s2 = Shelf { items: vec![g.as_str()] };
    println!("{}", s2.items[0]);
}
