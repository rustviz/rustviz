struct Part<'a> {
    part: &'a str,
}
struct Pair<'a, 'b> {
    l: &'a str,
    r: &'b str,
}
fn make(s: &str) -> Option<Part<'_>> {
    Some(Part { part: s })
}
fn main() {
    let s = String::from("hello");
    let o = Some(Part { part: &s });
    if let Some(x) = o {
        let q = x.part;
        println!("{}", q);
    }
    let t = String::from("world");
    match make(&t) {
        Some(m) => {
            println!("{}", m.part);
        }
        None => {}
    }
    let p = Some(Pair { l: &s, r: &t });
    if let Some(w) = p {
        println!("{}", w.l);
        println!("{}", w.r);
    }
}
