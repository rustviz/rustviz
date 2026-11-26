fn main() {
    let mut x = 0;
    let mut y = 0;
    let mut r = &mut x;
    let a = &mut *r;
    r = &mut y;
    *r = 1;
    *a = 2;
}

fn identity(r: &mut i32) -> &mut i32 { r }

// A call result can also replace the reference while a reborrow is live.
fn call_result() {
    let mut old = 0;
    let mut new = 0;
    let mut cursor = &mut old;
    let first = &mut *cursor;
    cursor = identity(&mut new);
    *cursor = 1;
    *first = 2;
}
