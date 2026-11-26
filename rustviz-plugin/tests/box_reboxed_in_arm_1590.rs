// #1590: `let s = *b;` moves only the String out of b's Box. What b holds
// later depends on the path, so the tooltips say only what rustc's move
// data guarantees: at a join, what may be uninitialized (`*b` or b) and
// that b can't be used until it's given a value; at a drop (b = .., *b =
// .., `?`, `return`, scope end), "whatever b still holds here (if
// anything) is dropped". Main said, in an arm, that b's whole previous
// resource is dropped "only if no branch above moved it", and at the join
// that b "may have no value". Also: b moved whole in the other branch
// (bw); a `?` after the join (bt); a `let bd;`; `*bp = …`; `bs = …; o?;`
// on one line; a new Box before the first `if` (br); the same in a loop
// (bl); `*bf = ..; bf = ..;`; `?` after `*bq = ..`; a `return` (be).
fn run(c: bool, d: bool) {
    let mut b = Box::new(String::from("ab"));
    if c {
        let _s = *b;
    }
    if d {
        b = Box::new(String::from("cd"));
    }
}

fn in_match_arm(c: bool, k: u8) {
    let mut bm = Box::new(String::from("ab"));
    if c {
        let _s = *bm;
    }
    match k {
        0 => bm = Box::new(String::from("cd")),
        _ => {}
    }
}

fn contents_moved_before(d: bool) {
    let mut bu = Box::new(String::from("ab"));
    let _s = *bu;
    if d {
        bu = Box::new(String::from("cd"));
    }
}

fn moved_whole_in_else(c: bool, d: bool) {
    let mut bw = Box::new(String::from("ab"));
    if c {
        let _s = *bw;
    } else {
        drop(bw);
    }
    if d {
        bw = Box::new(String::from("cd"));
    }
}

fn try_after_rebox(c: bool, d: bool, e: bool, v: Vec<u8>) -> Option<u8> {
    let mut bt = Box::new(String::from("ab"));
    if c {
        let _s = *bt;
    }
    if d {
        bt = Box::new(String::from("cd"));
    }
    if e {
    } else {
        let _n = v.last()?;
    }
    Some(0)
}

fn deferred(c: bool, d: bool) {
    let mut bd;
    if c {
        bd = Box::new(String::from("ab"));
        let _s = *bd;
    }
    if d {
        bd = Box::new(String::from("cd"));
    }
}

fn payload_write(c: bool, d: bool, e: bool) {
    let mut bp = Box::new(String::from("ab"));
    if c {
        let _s = *bp;
    }
    if d {
        bp = Box::new(String::from("cd"));
    }
    if e {
        *bp = String::new();
    }
}

fn same_line(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut bs = Box::new(String::from("ab"));
    if c {
        let _s = *bs;
    }
    if d {
        bs = Box::new(String::from("cd"));
    }
    bs = Box::new(String::new()); o?;
    Some(0)
}

fn replaced_first(c: bool, d: bool) {
    let mut br = Box::new(String::from("ab"));
    br = Box::new(String::new());
    if c {
        let _s = *br;
    }
    if d {
        br = Box::new(String::from("cd"));
    }
}

fn in_loop(c: bool, d: bool, n: u8) {
    let mut bl = Box::new(String::from("ab"));
    for _i in 0..n {
        bl = Box::new(String::new());
        if c {
            let _s = *bl;
        }
        if d {
            bl = Box::new(String::from("cd"));
        }
    }
}

fn refill_then_rebox(c: bool, d: bool, e: bool) {
    let mut bf = Box::new(String::from("ab"));
    if c {
        let _s = *bf;
    }
    if d {
        bf = Box::new(String::from("cd"));
    }
    if e {
        *bf = String::new(); bf = Box::new(String::new());
    }
}

fn refill_then_try(c: bool, d: bool, o: Option<u8>) -> Option<u8> {
    let mut bq = Box::new(String::from("ab"));
    if c {
        let _s = *bq;
    }
    if d {
        bq = Box::new(String::from("cd"));
    }
    *bq = String::new();
    o?;
    Some(0)
}

fn out_and_back(c: bool, o: Option<u8>) -> Option<u8> {
    let mut bo = Box::new(String::from("ab"));
    if c {
        let _s = *bo;
    } else {
        let t = bo;
        bo = t;
    }
    o?;
    Some(0)
}

fn early_return(c: bool, d: bool, e: bool) -> Option<u8> {
    let mut be = Box::new(String::from("ab"));
    if c {
        let _s = *be;
    }
    if d {
        be = Box::new(String::from("cd"));
    }
    if e {
        return None;
    }
    Some(0)
}

fn two_tries(o: Option<u8>, p: Option<u8>) -> Option<u8> {
    let mut b2 = Box::new(String::from("ab"));
    b2 = Box::new(o?.to_string()); p?;
    println!("{}", b2);
    Some(0)
}

// The join before `bz = Box::new(*bz);`: the right-hand side moves the
// contents out after it, not above it.
fn refilled_then_reboxed(c: bool) {
    let mut bz = Box::new(String::from("a"));
    if c {
        *bz = String::new();
    } else {
    }
    bz = Box::new(*bz);
    println!("{}", bz);
}

// Three `?`s on one line: each says which one it is.
fn three_tries(a: Option<u8>, b: Option<u8>, c: Option<u8>) -> Option<u8> {
    let mut s3 = String::new();
    println!("{}", s3);
    s3 = a?.to_string(); s3 = b?.to_string(); c?;
    println!("{}", s3);
    Some(0)
}

// The arm that moved the contents returns; the join (the function's
// end) is reached only through the other one, which keeps them.
fn returned_arm(c: bool) {
    let mut br2 = Box::new(String::from("a"));
    if c {
        let _s = *br2;
        return;
    } else {
        *br2 = String::new();
    }
}

// `?`s around an assignment, and one on a dot with other events.
fn tries_around(a: Option<u8>, b: Option<u8>, c: Option<u8>, d: Option<&str>) -> Option<u8> {
    let mut s4 = String::new();
    println!("{}", s4);
    a?; s4 = b?.to_string(); c?;
    s4 = a?.to_string(); s4.push_str(d?);
    println!("{}", s4);
    Some(0)
}
