// #1107: a boxed closure that reads through a shared reference
// (`|x| r.len() …`) reborrows `*r`, which rustc records as no loan. m
// still holds that borrow until it is dropped: a `Box<dyn Fn + '_>` has
// drop glue that may use it. So the borrow lasts until the closing brace,
// or until `drop(d)`, not just until m's last call.
fn boxed<'a>(f: impl Fn(&i32) -> bool + 'a) -> Box<dyn Fn(&i32) -> bool + 'a> {
    Box::new(f)
}

fn through_ref() -> bool {
    let v = String::from("d");
    let r = &v;
    let m = boxed(|x| r.len() == *x as usize);
    let hit = m(&1);
    hit
}

fn through_ref_plain() -> usize {
    let u = String::from("e");
    let q = &u;
    let n: Box<dyn Fn() -> usize + '_> = Box::new(|| q.len());
    let len = n();
    len
}

fn through_ref_dropped() -> bool {
    let mut w = String::from("f");
    let p = &w;
    let d = boxed(|x| p.len() == *x as usize);
    let found = d(&1);
    drop(d);
    w.push('g');
    found
}

fn main() {
    through_ref();
    through_ref_plain();
    through_ref_dropped();
}
