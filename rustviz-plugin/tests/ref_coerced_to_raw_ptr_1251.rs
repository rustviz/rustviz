// #1251: a reference coerced to a raw pointer (`let p: *mut i32 = &mut x;`)
// ends its borrow on the spot: a raw pointer carries no lifetime, so x can
// be used and assigned while p is in scope. Drawn like the explicit cast
// `&mut x as *mut i32` (#194): no borrow, p is a plain value.
struct S { p: *mut i32 }
fn main() {
    let mut x: i32 = 1;
    let p: *mut i32 = &mut x;
    unsafe { *p = 2; }
    let y = unsafe { *p };
    x = 7;
    let q: *const i32 = &x;
    x = 8;
    let mut m: *mut i32 = std::ptr::null_mut();
    m = &mut x;
    x = 9;
    let s = S { p: &mut x };
    x = 10;
    let mut z = String::from("z");
    let r: *const String = &z;
    z.push('!');
    println!("{} {} {:?} {:?} {:?} {:?} {}", x, y, q, m, s.p, r, z);
}
