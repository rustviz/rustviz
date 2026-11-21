// #1235: `match &t { s => … }` binds s to the reference `&t`: s borrows t
// until its last use, exactly as `let s = &t;` draws. Nothing is copied
// (t is a String) or moved (`match &mut t`).
struct P { name: String }
fn main() {
    let mut t = String::from("t");
    match &t { s => println!("{}", s) }
    match &mut t { s => s.push('!') }
    if let s = &t { println!("{}", s); }
    match (&t, 1) { (s, n) => println!("{} {}", s, n) }
    let v = vec![String::from("a")];
    match &v[0] { s => println!("{}", s) }
    let p = P { name: String::from("p") };
    match &p.name { s => println!("{}", s) }
    // A reference variable as the scrutinee: u is a copy of r (unchanged).
    let r = &t;
    match r { u => println!("{}", u) }
}

fn guarded() {
    let t = String::from("t");
    match &t { s if s.len() > 0 => println!("{}", s), _ => {} }
}
