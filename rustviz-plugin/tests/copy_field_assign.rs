// #178: assigning to a Copy place that already holds a value (`p.x = 5`,
// `n += 1`, `n = m`, a field of a Copy union) overwrites the value; it
// doesn't acquire ownership of anything or initialize the place.
#[derive(Clone, Copy)]
union Bits { i: u32, f: f32 }

struct P { x: i32, y: i32 }

fn main() {
    let mut p = P { x: 1, y: 2 };
    p.x = 5;
    let z = 7;
    p.y = z;
    let mut n = 1;
    n += 1;
    let m = 3;
    n = m;
    let mut b = Bits { i: 1 };
    b.f = 1.5;
    let k = unsafe { b.i };
    println!("{} {} {} {}", p.x, p.y, n, k);
}
