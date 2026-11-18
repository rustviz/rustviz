// The `?` operator returns early on `Err` / `None`, dropping what the
// function owns (#430). A value with no drop glue has nothing to drop
// there, so it gets no "only if the `?` returns early ... is dropped" dot
// (#1137): a `ManuallyDrop` (never drops what it holds), a struct, tuple
// or enum of Copy values, and closures that capture no resource (none,
// a Copy value by `move`, or a reference). A `String` or `Box` still gets
// its dot. The `?`s are in a `let`, an `if`, a loop body and a call
// argument. The same goes for the dot where a `return` hands the caller
// one of two ManuallyDrops: the other one drops nothing.

use std::mem::ManuallyDrop;

struct NoDrop(i32);

struct Mixed {
    s: String,
    n: NoDrop,
    m: ManuallyDrop<String>,
}

fn get() -> Option<i32> {
    Some(1)
}

fn demo(opt: Option<i32>, r: &String, c: bool) -> Option<i32> {
    let md = ManuallyDrop::new(String::from("md"));
    let nd = NoDrop(1);
    let pair = (NoDrop(2), 3);
    let maybe: Option<NoDrop> = None;
    let n = 5;
    let f = || 42;
    let g = move || n + 1;
    let k = || r.len();
    let mixed = Mixed { s: String::from("s"), n: NoDrop(4), m: ManuallyDrop::new(String::from("m")) };
    let b = Box::new(1);
    let v = opt?;
    if c {
        let w = get()?;
        println!("{}", w);
    }
    for i in 0..2 {
        get()?;
        println!("{}", i);
    }
    println!("{}", get()?);
    let _ = (&md, &pair, &maybe, &mixed);
    Some(v + nd.0 + f() + g() + k() as i32 + *b)
}

// A `return` of an `if` hands the caller x or y; the other one goes out
// of scope there. A ManuallyDrop drops nothing when it does.
fn pick(c: bool, d: bool) -> ManuallyDrop<String> {
    let x = ManuallyDrop::new(String::from("x"));
    let y = ManuallyDrop::new(String::from("y"));
    if c {
        return if d { x } else { y };
    }
    x
}

fn main() {
    let r = String::from("r");
    println!("{:?}", demo(Some(1), &r, true));
    println!("{}", *pick(true, false));
}
