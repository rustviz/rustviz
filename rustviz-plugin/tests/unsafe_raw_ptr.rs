// Minimal `unsafe` block exercising raw-pointer mutation. The
// visitor doesn't have explicit `unsafe`-block awareness; this
// fixture pins what *currently* happens (whether the plugin
// survives the input or panics) so any future change to unsafe
// handling shows up as a regression rather than a silent shift.

fn main() {
    let mut x: i32 = 1;
    let p: *mut i32 = &mut x as *mut i32;
    unsafe {
        *p = 2;
    }
}
