// RustViz 3 P1-4b review (#1860): a reference column keeps 2.x's style where
// the state table can't word it, and a closure's unique capture of a `&mut`
// is worded as one.

// `match &sb`: the shared loan is of the binding sb, not "shared through it".
fn shared_binding() {
    let mut n = 0;
    let mut sb = &mut n;
    match &sb {
        q => { let _v = **q; }
    }
    *sb += 1;
}

// A reborrow through ra or ra2 on one arm's path only.
fn one_arm(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let ra = &mut a;
    let ra2 = &mut b;
    let r = loop {
        if c { break ra; } else { break ra2; }
    };
    r.push('!');
}

// kept reborrows through w; w is then re-pointed on one path, so after the
// join it may point to h, which nothing shares.
fn repointed(c: bool) {
    let mut g = String::from("g");
    let mut h = String::from("h");
    let mut w = &mut g;
    let kept = &*w;
    if c {
        w = &mut h;
    }
    println!("{} {}", kept, w);
}

// The closure uniquely borrows p itself and writes through it.
fn unique() {
    let mut n = 1;
    let p = &mut n;
    let mut f = || *p += 1;
    f();
}

pub fn main() {
    shared_binding();
    one_arm(true);
    repointed(true);
    unique();
}
