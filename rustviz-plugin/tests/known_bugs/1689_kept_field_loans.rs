// Known bug, from the closed 2.x fix PRs (cx/cx-c18); kept as a RustViz 3 regression case.
// Issue(s): #1689 (see known_bugs/README.md for what main draws wrong).
struct Shelf<'a> { v: Vec<&'a i32> }

fn fields() {
    let mut e = (1, 2);
    let mut y = Vec::new();
    y.extend([(&e.0, &mut e.1)]);
    println!("{} {y:?}", e.0);
}
fn receiver() {
    let mut p = (1, 2);
    let mut shelf = Shelf { v: Vec::new() };
    shelf.v.extend([&p.0]);
    p.1 = 3;
    println!("{:?}", shelf.v);
}
fn ref_field() {
    let x = 1;
    let mut s = (&x, 2);
    let mut v: Vec<&&i32> = Vec::new();
    v.extend([&s.0]);
    s.1 = 3;
    println!("{v:?}");
}
trait Save<'a> {
    fn save<I: IntoIterator<Item = &'a i32>>(&mut self, it: I, extra: &'a i32);
}
impl<'a> Save<'a> for Vec<&'a i32> {
    fn save<I: IntoIterator<Item = &'a i32>>(&mut self, it: I, extra: &'a i32) {
        self.extend(it);
        self.push(extra);
    }
}
fn both() {
    let p = (1, 2);
    let extra = 3;
    let mut dst = Vec::new();
    dst.save([&p.0], &extra);
    println!("{dst:?}");
}
fn wrap<'a>(x: &'a i32, f: impl FnOnce() -> &'a mut i32) -> Option<(&'a i32, &'a mut i32)> {
    Some((x, f()))
}
fn with_closure() {
    let a = 1;
    let mut b = 2;
    let value = wrap(&a, || &mut b);
    println!("{a} {value:?}");
}
