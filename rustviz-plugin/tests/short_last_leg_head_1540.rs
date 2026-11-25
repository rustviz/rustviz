// #1540: an arrow whose last leg into its dot was shorter than the head's
// pull-back (a return arrow on a track a few px off its target dot's
// row) had its endpoint pulled back past the bend, and the head, drawn
// from the bend, pointed away from the dot and never reached it. The bend
// now moves back along the track so the last leg comes in at an angle,
// long enough for the head.
struct Holder<'a> { r: &'a String }
struct Pair { a: String, b: String }
impl Pair {
    fn get_a(&self) -> &String { &self.a }
}
fn holder<'a>(s: &'a String) -> Holder<'a> { Holder { r: s } }
fn into_else_leg(c: bool, d: bool) {
    let p = Pair { a: String::from("ab"), b: String::from("hey") };
    let mut s3 = String::from("ab");
    if c {
    } else if d {
        let _m = &mut s3;
        let _r = &p.b;
    } else {
        let _h2 = holder(p.get_a());
    }
}
// A return running left whose head pointed right (seed 101, bucket 05).
fn level_into_nested_if() {
    let s = String::from("rust");
    let r = &s;
    if s.len() > 1 {
    } else if s.len() > 1 {
        let _t = if s.len() > 1 { s.clone() } else { s };
    } else {
        println!("{}", r);
    }
}
fn main() {
    into_else_leg(true, false);
    level_into_nested_if();
}
