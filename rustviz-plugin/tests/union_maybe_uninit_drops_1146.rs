// A union never drops its fields (#191). One without a `Drop` impl of
// its own has no drop glue at all, inside a struct or not; one with a
// `Drop` impl runs it when the whole union is dropped or overwritten,
// though its fields are still not dropped. `MaybeUninit<T>` (a union in
// std) never drops the T it may hold, like a `ManuallyDrop<T>` (#1146).
// A `?` that returns early drops none of the no-glue ones.

use std::mem::MaybeUninit;

union U {
    a: i32,
    b: u32,
}

union D {
    a: i32,
    b: u32,
}

impl Drop for D {
    fn drop(&mut self) {}
}

struct Wrap {
    u: U,
    s: String,
}

struct WrapD {
    d: D,
}

fn make() -> MaybeUninit<String> {
    MaybeUninit::new(String::from("k"))
}

fn take_m(_m: MaybeUninit<String>) {}

fn take_u(_u: U) {}

fn get() -> Option<i32> {
    Some(1)
}

fn demo(c: bool) -> Option<i32> {
    let m: MaybeUninit<String> = MaybeUninit::new(String::from("m"));
    let mut m2 = make();
    let w = Wrap { u: U { a: 1 }, s: String::from("s") };
    let wd = WrapD { d: D { a: 1 } };
    let mut d = D { a: 1 };
    let u = U { b: 2 };
    let mu = MaybeUninit::<String>::uninit();
    let uu = U { a: 3 };
    get()?;
    m2 = MaybeUninit::new(String::from("m2"));
    d = D { a: 2 };
    d.a = 3;
    if c {
        take_m(mu);
        take_u(uu);
    }
    let _ = (&m, &m2, unsafe { w.u.a }, unsafe { wd.d.a }, unsafe { d.a }, unsafe { u.a });
    Some(1)
}

fn main() {
    let _ = demo(true);
}
