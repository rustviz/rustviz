// #194: raw pointers aren't modelled. `&mut x as *mut i32` draws no
// borrow of x, and a read or write through `*p` is drawn on p's column
// only; those dots say so. A write through a reference (`*r = 4`) is
// drawn on what r borrows and gets no note.
fn main() {
    let mut x: i32 = 1;
    let p = &mut x as *mut i32;
    unsafe {
        *p = 2;
    }
    let q = &x as *const i32;
    let y = unsafe { *q };
    let mut z = 3;
    let r = &mut z;
    *r = 4;
    println!("{} {} {}", x, y, z);
}
