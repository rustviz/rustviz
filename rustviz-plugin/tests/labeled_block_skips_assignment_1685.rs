fn main() {
    let s = String::from("a");
    let mut s2 = s;
    let c = s2.len() > 3;
    let t = s2;
    'blk: {
        if c { break 'blk; }
        s2 = String::from("b");
    }
    println!("{}", t);
}

fn destructuring(c: bool) {
    let mut pair_a = String::from("a");
    let mut pair_b = String::from("b");
    let old = (pair_a, pair_b);
    'block: {
        if c { break 'block; }
        (pair_a, pair_b) = (String::from("new a"), String::from("new b"));
    }
    drop(old);
}

fn both_paths_break(c: bool) {
    let mut either = String::from("a");
    let both_old = either;
    'block: {
        if c {
            either = String::from("b");
            break 'block;
        }
        break 'block;
    }
    drop(both_old);
}

fn reassign_after(c: bool) {
    let mut later = String::from("a");
    let old = later;
    'block: {
        if c { break 'block; }
        later = String::from("b");
    }
    later = String::from("c");
    println!("{later}");
    drop(old);
}

fn try_after(c: bool, r: Result<(), ()>) -> Result<(), ()> {
    let mut pending = String::from("a");
    let old = pending;
    'block: {
        if c { break 'block; }
        pending = String::from("b");
    }
    r?;
    drop(old);
    Ok(())
}
