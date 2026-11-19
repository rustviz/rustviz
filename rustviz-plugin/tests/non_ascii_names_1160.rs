// #1160: non-ASCII identifiers (valid Rust since 1.53) and non-ASCII
// text before a name on its line. They used to panic or shift the
// code panel's highlights by a byte.
struct Punkt { größe: i32 }
enum Farbe { Grün(i32), Blau }
fn länge(s: String) -> usize { s.len() }
fn main() {
    let café = String::from("naïve"); let t = café;
    let p = Punkt { größe: 3 };
    println!("é → {} {}", t, p.größe); // emoji 🦀 in a comment
    let n = länge(t);
    let 名前 = String::from("日本語 🦀"); let r = &名前;
    println!("{} {}", n, r);
    let f = Farbe::Grün(1);
    if let Farbe::Grün(x) = f { println!("{}", x); }
    let _b = Farbe::Blau;
}
