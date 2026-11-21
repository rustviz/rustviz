// #689: a write or read through a reference stored in an element
// (`*slots[0] += 10`) is drawn on the collection's column; which
// variable the element borrows from isn't followed. Those dots say so.
// An element write that isn't through a reference (`v[0] = 5`) or one
// through a Box (`*boxes[0] = 7`) gets no note.
fn main() {
    let mut x = 1;
    let mut slots: Vec<&mut i32> = vec![&mut x];
    *slots[0] += 10;
    let n = *slots[0];
    let mut v = vec![1, 2];
    v[0] = 5;
    let mut boxes = vec![Box::new(1)];
    *boxes[0] = 7;
    println!("{} {:?} {:?}", n, v, boxes);
}
