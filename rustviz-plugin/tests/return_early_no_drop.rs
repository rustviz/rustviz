// #318 with #484: `Pt` isn't Copy but has nothing to drop, so on the
// path that skipped the `return`, `p` "still owns its value", not a
// resource.
struct Pt {
    x: i32,
}

fn pick(n: i32) -> Pt {
    let p = Pt { x: 1 };
    if n > 0 {
        return p;
    }
    let q = Pt { x: 2 };
    q
}

fn main() {
    let r = pick(1);
    println!("{}", r.x);
}
