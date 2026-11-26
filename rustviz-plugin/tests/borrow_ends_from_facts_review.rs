// RustViz 3 P2-1b review round (#1872): borrow ends from the facts in
// cases the first version got wrong. Pinned by
// `borrow_ends_from_facts_review_ids` in corpus.rs.

// b's loan is taken on a's last-use row, before that use: s stays
// borrowed by b after a's end (rustc rejects `s.clear()` between the two
// prints), so a's end gives nothing back.
fn same_row() {
    let s = String::from("s");
    let a = &s;
    println!("{}", a);
    let b = &s; println!("{}", a);
    println!("{}", b);
}

// Two loops whose bindings share a name: the first body's `drop(x)` ends
// only the first x's borrow; the second x gives w back where its loop
// ends.
fn two_loops() {
    let mut v = vec![String::from("a")];
    let mut w = vec![String::from("b")];
    for x in v.iter_mut() {
        drop(x);
    }
    for x in w.iter_mut() {
        x.push('!');
    }
    println!("{:?} {:?}", v, w);
}

// A copy of r holds the loan on to a call that never returns: r's own
// borrow ends at the copy, and q gives s back at the call, before s is
// dropped there.
fn never(r: &String) -> ! {
    panic!("{}", r)
}

fn never_copy() {
    let s = String::from("s");
    let r = &s;
    let q = r;
    println!("{}", s.len());
    never(q);
}

// Two sibling blocks each declare an `r`: `w.len()` in the first is a
// call on w while the first r still borrows it.
fn name_reuse() {
    let w = vec![String::from("w")];
    {
        let r = &w;
        println!("{}", w.len());
        println!("{}", r[0]);
    }
    {
        let r = &w;
        println!("{}", r.len());
    }
}

fn main() {
    same_row();
    two_loops();
    name_reuse();
    never_copy();
}
