// A reference whose lender has no column but is 'static data: the
// borrow-end arrow names it. `s` and `t` (a copy of `s`) borrow a string
// literal, `b` a byte string literal, `g` the static COUNT itself; `h` and
// `c` are copies of a static's / const's reference, so they borrow what
// that reference points to. `w`, a call result, keeps the generic text.
static GREETING: &str = "hello";
static COUNT: i32 = 3;
const NAME: &str = "rv";

fn pick(x: &str) -> &str { x } // rustviz: hide

fn main() {
    let s: &str = "hi";
    let b: &[u8; 2] = b"hi";
    let g = &COUNT;
    let h = GREETING;
    let c = NAME;
    let t = s;
    let w = pick(t);
    println!("{} {:?} {} {} {} {} {}", s, b, g, h, c, t, w);
}
