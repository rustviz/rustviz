// #897: `*r = <reference>` with `r: &mut &T` re-points a reference
// variable only when r borrows one (`let r = &mut s; *r = "c";`).
// Otherwise there is no variable to re-point: the write replaces the
// reference stored where r points, drawn like `*r = 5` through
// `r: &mut i32` ("*r is assigned a new value"). `&str` needs no drop.

// r points to the caller's `s`.
fn set(r: &mut &str) {
    *r = "x";
}

fn main() {
    let mut s = "a";
    set(&mut s);
    println!("{}", s);
}

// w points to an element. (Like `*w = 7` with `Vec<i32>`, the write isn't
// drawn on words's column, #861.)
fn get_mut_elem() {
    let mut words = vec!["a", "b"];
    if let Some(w) = words.get_mut(1) {
        *w = "c";
    }
    println!("{:?}", words);
}

fn index_elem() {
    let mut names = vec!["a", "b"];
    let e = &mut names[1];
    *e = "c";
    println!("{:?}", names);
}

// `*p = &t` stores a borrow of t in refs[0]: t stays borrowed until refs'
// last use, and so does `a`, borrowed through refs' type.
fn store_borrow() {
    let a = String::from("a");
    let t = String::from("t");
    let mut refs = vec![&a];
    let p = &mut refs[0];
    *p = &t;
    println!("{:?}", refs);
}

// q points to an element of tags through the reference v: v keeps
// pointing to tags.
fn through_ref() {
    let mut tags = vec!["a", "b"];
    let v = &mut tags;
    let q = &mut v[1];
    *q = "c";
    println!("{:?}", tags);
}

// A reborrow: *m is z, reached through x; x keeps pointing to z.
fn reborrow() {
    let mut z = "a";
    let x = &mut z;
    let m = &mut *x;
    *m = "c";
    println!("{}", z);
}
