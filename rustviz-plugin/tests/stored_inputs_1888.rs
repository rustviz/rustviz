fn pick<'a>(p: &'a mut &'a String, q: &'a &'a String, flag: bool) -> &'a &'a String {
    if flag { p } else { q }
}

fn pick_shared<'a>(p: &'a &'a String, q: &'a &'a String, flag: bool) -> &'a &'a String {
    if flag { p } else { q }
}

fn repeated(flag: bool) {
    let t = String::from("two");
    let s = String::from("one");
    let mut p = &s;
    let q = &t;
    let mut nested = Vec::new();
    nested.push(&q);
    nested.push(
        pick(&mut p, &q, flag),
    );
    println!("{} {}", t.len(), nested[1].len());
}

fn shared(flag: bool) {
    let a = String::from("one");
    let b = String::from("two");
    let p = &a;
    let q = &b;
    let mut refs = Vec::new();
    refs.push(pick_shared(&p, &q, flag));
    println!("{refs:?}");
}

fn captures(p: &mut i32, mut k: i32) {
    let mut callbacks = Vec::new();
    callbacks.push(|| { let _s = &p; *p += 1; k += 1; });
    callbacks[0]();
}

trait Inspect {
    fn inspect(&mut self, s: &String);
}

impl Inspect for Vec<&String> {
    fn inspect(&mut self, s: &String) { println!("{s}"); }
}

fn inspect_previously_stored(s: String) {
    let r = &s;
    let mut h = Vec::new();
    h.push(r);
    h.inspect(r);
    println!("{h:?}");
}
