// #880: a `&` pattern matched against a reference variable itself (not a
// reference stored inside a value, #874) matches the reference, and its
// binding gets a copy of the value the reference points to: `let &b = r;`
// is `let b = *r;`.
fn next(r: &i32) -> i32 {
    match r {
        &0 => 0,
        &v => v + 1,
    }
}

fn main() {
    let n = 5;
    let r = &n;
    let &b = r;
    match r {
        &c => println!("{}", c),
    }
    if let &w = r {
        println!("{}", w);
    }
    let t = (1, 2);
    let rt = &t;
    let &(x, y) = rt;
    let o = Some(3);
    let ro = &o;
    if let &Some(q) = ro {
        println!("{}", q);
    }
    let mut m = 7;
    let rm = &mut m;
    let &mut e = rm;
    println!("{} {} {} {} {}", b, x, y, e, next(&n));
}
