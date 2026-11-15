// #921: moving a Box's contents out (`let w = *e;`) in some branches
// moves *e, not e: e still owns its heap allocation on every path. After
// the join rustc rejects any use of e (E0382) except giving *e or e a
// new value (with `let mut`). On the path that didn't move them, the
// contents are dropped by that next `*e = …` / `e = …`, or else at e's
// scope end; the allocation is freed at e's scope end or reassignment
// either way. The join said "e was moved in at least one branch above".

struct P { a: i32 }
fn take(_p: P) {}

fn main() {
    let e = Box::new(String::from("a"));
    let k = 3;
    if k == 0 {
        let w = *e;
        drop(w);
    } else {
        println!("no");
    }

    // `let mut`, then a new value for *e: the old String, if still
    // there, is dropped by that write.
    let mut f = Box::new(String::from("f"));
    if k == 1 {
        let w = *f;
        drop(w);
    }
    *f = String::from("f2");
    println!("{}", f);

    // A new Box for g: the old String, if still there, and the old
    // Box's allocation are dropped by that assignment.
    let mut g = Box::new(String::from("g"));
    if k == 2 {
        let w = *g;
        drop(w);
    }
    g = Box::new(String::from("g2"));
    println!("{}", g);

    // Contents that need no drop: only the allocation is freed.
    let b = Box::new(P { a: 1 });
    match k {
        4 => take(*b),
        _ => {}
    }
}
