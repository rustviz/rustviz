// Compound assignment to a field through a reference (#147, #260):
// `p.x += k` updates the struct p points to in place, so it reads like
// `*r += k` ("modified through"), not like an assignment, and it
// doesn't say "(copied from k)": the arrow from k says that.
struct P {
    x: i32,
    name: String,
}

fn bump(p: &mut P, k: i32) {
    p.x += k;
    p.name += "x";
}

fn main() {
    let mut p = P { x: 1, name: String::from("a") };
    let k = 2;
    bump(&mut p, k);
    let r = &mut p;
    r.x += k;
    println!("{} {}", p.x, p.name);
}
