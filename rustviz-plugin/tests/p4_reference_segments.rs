#![allow(dead_code)]

struct Pair { left: i32, right: i32 }

fn regions<'a>(mut r: &'a mut Pair, next: &'a mut Pair) {
    let part = &mut r.left;
    r.right += 1;
    *part += 2; r = next;
    r.right += 3;
}

fn slot<'a>(mut r: &'a mut Pair, next: &'a mut Pair) {
    let held = &mut r;
    (**held).right += 1;
    r = next;
    r.right += 2;
}

fn repoint<'a>(mut r: &'a mut Pair, next: &'a mut Pair) {
    let old = &mut *r; r = next;
    r.right += 1;
    old.left += 1;
}

fn arms(r: &mut Pair, choice: u8) {
    match choice {
        0 => {
            let field = &mut r.left;
            r.right += 1;
            *field += 2;
        }
        1 => {
            let shared = &*r;
            let read = shared.right;
            assert_eq!(shared.left, read);
        }
        _ => { r.right += 4; }
    }
    r.right += 8;
}

fn two_stores<'a>(mut r: &'a mut Pair, next: &'a mut Pair, last: &'a mut Pair) {
    let before = &mut *r;
    r = next; let middle = &mut r.left; r = last;
    r.left += 1;
    *middle += 1;
    before.left += 1;
}

fn joined(r: &mut Pair, other: &mut Pair, flag: bool) {
    let dst: &mut Pair;
    if flag { dst = &mut *r; } else { dst = &mut *other; }
    dst.left += 1;
    r.right += 1;
}
