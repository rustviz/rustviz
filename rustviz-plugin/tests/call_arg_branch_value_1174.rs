// #1174: an `if` / `match` written as a call's argument. The value each
// branch yields is passed to the call on the path through that branch:
// `a` on one, `b` on the other, and an arm's binding `x` is passed to the
// call, not dropped at the arm's end. Also through a method call, a
// constructor (`Some(..)`), and a constructor returned to the caller.
fn take(_s: String) {}
fn take_opt(_o: Option<String>) {}
fn wrap(o: Option<String>, d: String) -> Option<String> {
    Some(match o { Some(x) => x, None => d })
}
fn main() {
    let c = true;
    let a = String::from("a");
    let b = String::from("b");
    take(if c { a } else { b });
    let o = Some(String::from("o"));
    let d = String::from("d");
    take(match o { Some(y) => y, None => d });
    let mut v = Vec::new();
    let e = String::from("e");
    let f = String::from("f");
    v.push(if c { e } else { f });
    let g = String::from("g");
    let h = String::from("h");
    take_opt(Some(if c { g } else { h }));
    let w = wrap(None, String::from("w"));
    println!("{:?} {:?}", v, w);
}
