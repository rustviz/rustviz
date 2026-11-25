// #1541: the fields still in a struct are moved in every arm of a
// conditional. The struct's own line stops where those fields fork into
// the arms: past there nothing of it can be used, and the fields' arm
// legs show what's left of it.
struct Pair {
    a: String,
    b: String,
}

struct Three {
    a: String,
    b: String,
    c: String,
}

fn moved_in_if(p: Pair, c: bool) {
    let s3 = p.b;
    if c {
        drop(p.a);
    } else {
        drop(p.a);
    }
    println!("{}", s3);
}

fn moved_in_match(q: Pair, n: i32) {
    let s3 = q.b;
    match n {
        0 => drop(q.a),
        _ => drop(q.a),
    }
    println!("{}", s3);
}

fn two_left(t: Three, c: bool) {
    let x = t.a;
    if c {
        drop(t.b);
        drop(t.c);
    } else {
        let y = (t.c, t.b);
        drop(y);
    }
    println!("{}", x);
}

// Only one of the two fields left is moved in the arms: t.c can still be
// used, so u's line goes on.
fn one_left(u: Three, c: bool) {
    let x = u.a;
    if c {
        drop(u.b);
    } else {
        drop(u.b);
    }
    println!("{} {}", x, u.c);
}

fn main() {
    moved_in_if(Pair { a: String::new(), b: String::new() }, true);
    moved_in_match(Pair { a: String::new(), b: String::new() }, 0);
    two_left(Three { a: String::new(), b: String::new(), c: String::new() }, true);
    one_left(Three { a: String::new(), b: String::new(), c: String::new() }, true);
}
