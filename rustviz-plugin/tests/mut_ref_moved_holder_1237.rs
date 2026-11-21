// #1237: after a `&mut` is moved on, into another reference (`let b = a;`)
// or a generic field (`G { f: r }`, `W(q)`: no reborrow), the lender's
// segment names the new holder from that line. An `&'a mut` field
// (`H { f: c }`) reborrows c, which stays the holder.
struct G<T> { f: T }
struct W<T>(T);
struct H<'a> { f: &'a mut String }
fn main() {
    let mut s = String::from("hello");
    let r = &mut s;
    let w = G { f: r };
    let _spacer = 1;
    w.f.push('!');
    let mut t = String::from("t");
    let q = &mut t;
    let x = W(q);
    let _spacer2 = 1;
    x.0.push('!');
    let mut u = String::from("u");
    let a = &mut u;
    let b = a;
    let _spacer3 = 1;
    b.push('!');
    let mut v = String::from("v");
    let c = &mut v;
    let h = H { f: c };
    let _spacer4 = 1;
    h.f.push('!');
}
