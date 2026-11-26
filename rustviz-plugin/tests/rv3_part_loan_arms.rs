struct Pair { a: String, b: String }
fn inspect(_: &Pair) {}

fn choose(mut p: Pair, left: bool) {
    let r = if left { inspect(&p); &mut p.a } else { inspect(&p); &mut p.b };
    r.clear();
    inspect(&p);
}

fn in_arm(mut p: Pair, left: bool) {
    if left {
        inspect(&p);
        let r = &mut p.b;
        p.a.clear();
        r.clear();
        inspect(&p);
    } else {
        inspect(&p);
    }
}

struct Handles<'a> { raw: *mut i32, shared: &'a &'a i32 }
fn indirect(mut h: Handles<'_>) {
    let raw = unsafe { &mut *h.raw };
    let shared = &**h.shared;
    let _ = &mut h;
    *raw += *shared;
}

fn moved_holder(mut p: Pair) {
    let r = &mut p.a;
    let next = r;
    let _ = &mut p.b;
    next.clear();
    inspect(&p);
}

fn first(p: &mut Pair) -> &mut String { &mut p.a }
fn whole_or_part(mut p: Pair, left: bool) {
    let r = if left { first(&mut p) } else { &mut p.b };

    r.clear();
    inspect(&p);
}
