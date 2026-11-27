pub fn correlated_condition(assign: bool) {
    let s: String;
    if assign {
        s = String::new();
        if !assign {
            drop(s);
        }
    }
    let _after = 0;
}

pub fn imaginary_move(assign: bool, t: (i32,)) {
    let s: String;
    if assign {
        s = String::new();
        if let (_,) = t {
        } else {
            drop(s);
        }
    }
    let _after = 0;
}

struct Pair {
    taken: String,
    spare: String,
}

pub fn whole_move(assign: bool, take: bool, t: (i32,)) {
    let p: Pair;
    if assign {
        p = Pair { taken: String::new(), spare: String::new() };
        if take {
            drop(p);
        }
    }
    if let (_,) = t {
    } else {
        let _dead = 0;
    }
}

pub fn sibling_move(assign: bool, take: bool, t: (i32,)) {
    let p: Pair;
    if assign {
        p = Pair { taken: String::new(), spare: String::new() };
        if take {
            drop(p.taken);
        }
    }
    if let (_,) = t {
    } else {
        let _dead = 0;
    }
}

fn fail() -> String {
    panic!("before reassignment");
}

pub fn panic_before_reassign(assign: bool, consume: bool) {
    let mut s: String;
    if assign {
        s = String::new();
        if consume {
            drop(s);
        }
    }
    s = fail();
    drop(s);
}
