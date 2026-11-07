// #438: reading through a reference (or Box) stored in a collection
// copies out of what the element points to, not out of the
// collection. `let n = *slots[0];` copies the i32 that the `&mut i32`
// in slots[0] points to (x); slots is a Vec, not a reference. Which
// variable an element points to isn't known statically, so the read
// is drawn as an arrow out of slots's column and names the element.
struct Holder<'a> {
    r: &'a i32,
}

fn main() {
    let mut x = 1;
    let slots = vec![&mut x];
    let n = *slots[0];
    let a = 2;
    let hs = vec![Holder { r: &a }];
    let m = *hs[0].r;
    let boxes = vec![Box::new(5)];
    let k = *boxes[0];
    let b = 3;
    let grid = vec![vec![&b]];
    let mut total = 0;
    total = *grid[0][0];
    println!("{} {} {} {} {:?}", n, m, k, total, slots);
}
