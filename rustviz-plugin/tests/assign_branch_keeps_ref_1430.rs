// #1430: `best = if c { &b } else { best };` (a reference given a new
// value by a conditional, one branch giving it back its own value). On that
// path best keeps its loan of a, so the write ends none of it: a and b stay
// borrowed until best's last use (rustc keeps both, as for `if c { best =
// &b; }`), and the branch that keeps best draws nothing. best's old loan
// used to end before the conditional, a was given back twice, and the else
// branch drew "Copy from best to best".
fn main() {
    let a = String::from("a");
    let b = String::from("bb");
    let e = String::from("eee");
    let mut best = &a;
    best = if b.len() > best.len() { &b } else { best };
    println!("{}", best);
    best = match e.len() { 3 => &e, _ => best };
    println!("{}", best);
    best = if a.len() > 5 { &a } else if b.len() > 5 { &b } else { best };
    println!("{}", best);
    let mut s = String::from("s");
    let mut t = String::from("t");
    let mut m = &mut s;
    m.push('!');
    m = if t.len() > 0 { &mut t } else { m };
    m.push('?');
    println!("{} {}", s, t);
}
