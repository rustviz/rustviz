// #1568: a struct's reference field holding a copy of another reference
// ends where the struct is moved or last used, before the struct's scope
// end, also in a loop's body or when the copied-from field is re-pointed
// later. Main drew its end after its column had ended.
struct Holder<'a> {
    r: &'a String,
}

struct Counted<'a> {
    r: &'a String,
    n: i32,
}

fn in_loop_arm() {
    let s = String::from("ab");
    let r = &s;
    for i in 0..3 {
        if i > 0 {
            let h = Holder { r };
            drop(h);
        } else {
        }
    }
}

fn base_then_repoint(b: bool) {
    let s1 = String::from("ab");
    let mut c = Counted { r: &s1, n: 1 };
    if b {
        let c2 = Counted { n: 2, ..c };
        drop(c2);
    }
    c.r = &s1;
    println!("{}", c.n);
}

fn param_in_loop(p: &String) {
    loop {
        let k = Counted { r: p, n: 1 };
        break;
    }
}

fn counted<'a>(e: &'a String) -> Counted<'a> {
    Counted { r: e, n: 0 }
}

fn base_in_loop_else() {
    let e = String::from("ab");
    let g = counted(&e);
    let mut n = 0;
    while n < 3 {
        if n > 0 {
        } else {
            let g2 = Counted { n: 2, ..g };
            drop(g2);
        }
        n += 1;
    }
}

fn main() {
    in_loop_arm();
    base_then_repoint(true);
    let t = String::from("x");
    param_in_loop(&t);
    base_in_loop_else();
}
