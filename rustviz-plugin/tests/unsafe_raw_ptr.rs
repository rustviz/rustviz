// Minimal `unsafe` block exercising raw-pointer mutation. The
// visitor doesn't have explicit `unsafe`-block or raw-pointer
// awareness; this fixture pins the *current*, known-imperfect
// output so any future change to unsafe handling shows up as a
// regression rather than a silent shift. Today the write `*p = 2`
// (really a write to x) is drawn as the start of p's column ("*p is
// bound to a value"), and the `let p = &mut x as *mut i32` line
// shows nothing at all: no borrow of x, no binding of p.

fn main() {
    let mut x: i32 = 1;
    let p: *mut i32 = &mut x as *mut i32;
    unsafe {
        *p = 2;
    }
}
