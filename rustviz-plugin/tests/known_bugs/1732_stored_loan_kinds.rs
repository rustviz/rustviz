// Known bug, from the closed 2.x fix PRs (cx/cx-c18); kept as a RustViz 3 regression case.
// Issue(s): #1732 (see known_bugs/README.md for what main draws wrong).
fn choose<'a>(x: &'a mut String, y: &'a String, f: bool) -> &'a String {
    if f { x } else { y }
}

fn id(s: &String) -> &String { s }
fn first<'a>(v: &Vec<&'a String>) -> &'a String { v[0] }

fn kept() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let r: &String = &mut a;
    let mut v = Vec::new();
    v.extend([(r, &mut b)]);
    println!("{v:?}");
}

fn several(flag: bool) {
    let mut c = String::from("c");
    let d = String::from("d");
    let r = choose(&mut c, &d, flag);
    let mut dest = Vec::new();
    dest.push(id(r));
    println!("{d} {dest:?}");
}

fn holders(flag: bool) {
    let mut e = String::from("e");
    let g = String::from("g");
    let r: &String = &mut e;
    let x = vec![r];
    let y = vec![&g];
    let rx = if flag { &x } else { &y };
    let mut w = Vec::new();
    w.push(first(rx));
    println!("{g} {w:?}");
}

trait Store<'a> {
    fn store(&mut self, a: &'a mut String, b: &'a String);
}
impl<'a> Store<'a> for Vec<&'a String> {
    fn store(&mut self, a: &'a mut String, b: &'a String) {
        self.push(a);
        self.push(b);
    }
}

fn storing() {
    let mut j = String::from("j");
    let k = String::from("k");
    let mut v = Vec::new();
    v.store(&mut j, &k);
    let mut out = Vec::new();
    out.push(first(&v));
    println!("{k} {out:?}");
}

fn pick<'a>(x: &'a mut &'a String, y: &'a &'a String, f: bool) -> &'a &'a String {
    if f { x } else { y }
}

fn slots(flag: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let mut p = &s;
    let q = &t;
    let mut nested = Vec::new();
    nested.push(pick(&mut p, &q, flag));
    println!("{s} {t} {nested:?}");
}
