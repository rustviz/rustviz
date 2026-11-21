// #1220: a Box whose contents were moved out (`let _s = *b;`) and that a
// branch then gives a new Box (and maybe moves away) holds something
// different on each path: the emptied Box (only its heap allocation is
// freed), the new Box (dropped whole), or nothing. Rust's drop flags
// decide; the scope end, the join and a later reassignment say so.
fn moved_new(c: bool) {
    let mut b = Box::new(String::from("a"));
    let _s = *b;
    if c {
        b = Box::new(String::from("b"));
        drop(b);
    }
}

fn kept_new(c: bool) {
    let mut f = Box::new(String::from("a"));
    let _s = *f;
    if c {
        f = Box::new(String::from("b"));
        println!("{}", f);
    }
}

fn three(n: i32) {
    let mut g = Box::new(String::from("a"));
    let _s = *g;
    match n {
        0 => {
            g = Box::new(String::from("b"));
            drop(g);
        }
        1 => {
            g = Box::new(String::from("c"));
            println!("{}", g);
        }
        _ => {}
    }
}

fn reassigned(c: bool) {
    let mut j = Box::new(String::from("a"));
    let _s = *j;
    if c {
        j = Box::new(String::from("b"));
    }
    j = Box::new(String::from("d"));
    println!("{}", j);
}

fn main() {
    moved_new(true);
    kept_new(false);
    three(2);
    reassigned(true);
}
