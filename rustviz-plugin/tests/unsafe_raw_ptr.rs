// Minimal `unsafe` block exercising raw-pointer mutation. The
// visitor doesn't have explicit `unsafe`-block or raw-pointer
// awareness; this fixture pins the *current*, known-imperfect
// output so any future change to unsafe handling shows up as a
// regression rather than a silent shift. Today the write `*p = 2`
// reads "the resource p points to is modified through *p", but the
// `let p = &mut x as *mut i32` line shows no borrow of x (#194).

fn main() {
    let mut x: i32 = 1;
    let p: *mut i32 = &mut x as *mut i32;
    unsafe {
        *p = 2;
    }
}
