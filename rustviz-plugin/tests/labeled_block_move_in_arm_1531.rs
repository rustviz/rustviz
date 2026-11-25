// #1531: a labeled block in an arm breaks before a &mut is moved.
fn local(c: bool, d: bool) {
    let mut s = String::new();
    let m = &mut s;
    if c {
        'blk: {
            if d { break 'blk; }
            drop(m);
        }
    } else {
        drop(m);
    }
}
fn take_gen<T>(_t: T) {}
fn param(m: &mut String, c: bool, d: bool) {
    if c {
        'blk: {
            if d { break 'blk; }
            take_gen(m);
        }
    } else {
        take_gen(m);
    }
}
fn main() {}
