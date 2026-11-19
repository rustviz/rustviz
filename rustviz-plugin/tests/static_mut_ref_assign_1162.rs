// #1162: assigning to a reference-typed `static mut` panicked. A static
// can only hold a `&'static`: a string literal, another static, a
// promoted constant. Nothing is borrowed from a local and nothing is
// dropped.
static OTHER: &str = "other";
static mut NAME: &str = "a";
static mut NUMS: &[i32] = &[1, 2];

fn set(v: &'static str) {
    unsafe { NAME = v; }
}

fn main() {
    unsafe { NAME = "b"; }
    let lit: &'static str = "z";
    set(lit);
    unsafe {
        if lit.len() > 0 { NAME = OTHER; }
        for _ in 0..2 { NAME = "c"; }
        NUMS = &[3, 4, 5];
    }
    let n = unsafe { NAME };
    let k = unsafe { NUMS };
    println!("{} {}", n, k.len());
}
