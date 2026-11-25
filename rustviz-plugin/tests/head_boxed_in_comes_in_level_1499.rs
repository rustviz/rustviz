// #1499: s2's dot on vr's line is boxed in: s2 lends to vr level along
// the row, r's return runs half a row above and s's loan of h.r half a
// row below. vr's return comes round past s2 and into its dot from the
// left, nearly level, its head clear of every other arrow.
struct Holder<'a> {
    r: &'a String,
    n: i32,
}

struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn make_pair() -> Pair {
    Pair { a: make(), b: make() }
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let mut s = String::from("abc");
    let mut s2 = String::from("ab");
    let o = Some(make());
    let r = &s;
    if o.is_some() {
    } else {
        if let Some(x2) = o.as_ref() {
            Pair { a: s2, b: s } = make_pair();
            let vr = vec![s2.as_str(), "hi"];
            let h = holder(downgrade(&mut s));
        } else {
            let r3 = r;
        }
    }
}
