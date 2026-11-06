// #513: in `point(&mut hold, &t)` (line 22), point's signature
// `fn point<'a>(h: &mut H<'a>, t: &'a String)` gives `t` the lifetime
// of the reference an `H<'a>` holds, so point may store `&t` in hold
// (it does: `h.r = t`). The borrow checker goes by the signature, so t
// stays borrowed until hold's last use (line 23), not just until
// point returns. `look` gives its `t` a lifetime of its own, so u's
// borrow ends at that call. A `&mut self` method (`set`) and a
// `&'a mut` argument (`put`) work the same way.
struct H<'a> { r: &'a String }
impl<'a> H<'a> {
    fn set(&mut self, t: &'a String) { self.r = t; }
}
fn point<'a>(h: &mut H<'a>, t: &'a String) { h.r = t; }
fn look(h: &mut H<'_>, t: &String) { println!("{} {}", h.r, t); }

fn main() {
    let s = String::from("s");
    let t = String::from("t");
    let u = String::from("u");
    let mut hold = H { r: &s };
    look(&mut hold, &u);
    point(&mut hold, &t);
    println!("{}", hold.r);
    let mut h2 = H { r: &u };
    h2.set(&s);
    println!("{}", h2.r);
}

struct M<'a> { r: &'a mut String }
fn put<'a>(m: &mut M<'a>, x: &'a mut String) { m.r = x; }

fn mutable() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut m = M { r: &mut a };
    put(&mut m, &mut b);
    m.r.push('!');
    println!("{}", b);
}
