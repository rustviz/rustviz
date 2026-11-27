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
    let mut a = 1;
    let mut b = 2;
    let value = wrap(&a, || &mut b);
    println!("{a} {value:?}");
    let _ = &mut b;
    let _ = &mut a;
}

fn closure_may_go_unused() {
    fn ignore<'a>(_: impl FnOnce() -> &'a mut i32) -> Option<&'a mut i32> { None }
    let mut ignored = 2;
    let h = ignore(|| &mut ignored);
    assert!(h.is_none());
    let _ = &mut ignored;
}

fn copyable_closure_results() {
    fn result<'a>(f: impl FnOnce() -> &'a i32) -> Result<&'a i32, ()> { Ok(f()) }
    fn option<'a>(f: impl FnOnce() -> &'a i32) -> Option<&'a i32> { Some(f()) }
    let mut shared = 2;
    let r = result(|| &shared);
    let opt = option(|| &shared);
    let copied_r = r;
    let copied_opt = opt;
    println!("{r:?} {opt:?} {copied_r:?} {copied_opt:?}");
    let _ = &mut shared;
}

fn move_closure_result() {
    let shared_a = 1;
    let mut exclusive_b = 2;
    let original = wrap(&shared_a, || &mut exclusive_b);
    let moved = original;
    println!("{shared_a} {moved:?}");
    let _ = &mut exclusive_b;
}

fn keep<'a, T>(f: impl FnOnce() -> &'a mut T) -> Option<&'a mut T> { Some(f()) }

fn assign_closure_result() {
    let mut assigned = 2;
    let mut replacement = None;
    assert!(replacement.is_none());
    replacement = keep(|| &mut assigned);
    println!("{replacement:?}");
    let _ = &mut assigned;
}

fn branch_closure_results(flag: bool) {
    let mut left = 1;
    let mut right = 2;
    let joined = if flag {
        keep(|| &mut left)
    } else {
        keep(|| &mut right)
    };
    println!("{joined:?}");
    let _ = (&mut left, &mut right);
    let mut maybe = 3;
    let maybe_ref = if flag {
        keep(|| &mut maybe)
    } else {
        None
    };
    println!("{maybe_ref:?}");
    let _ = &mut maybe;
}

fn closure_borrows_reference_slot() {
    let x = 1;
    let y = 2;
    let mut slot = &x;
    let slot_holder = keep(|| &mut slot);
    println!("{slot_holder:?}");
    slot = &y;
    println!("{slot}");
}

struct Slot<'a, 'b> { item: &'a mut i32, other: &'b i32 }
fn package<'a, 'b>(f: impl FnOnce() -> &'a mut i32, other: &'b i32) -> Slot<'a, 'b> {
    Slot { item: f(), other }
}
fn package_tuple<'a>(f: impl FnOnce() -> &'a mut i32) -> (bool, Option<&'a mut i32>) {
    (true, Some(f()))
}
fn closure_result_fields() {
    let mut item = 1;
    let other = 2;
    let packaged = package(|| &mut item, &other);
    *packaged.item += 1;
    assert_eq!(*packaged.other, 2);
    assert_eq!(item, 2);
    let mut element = 3;
    let tuple = package_tuple(|| &mut element);
    println!("{:?}", tuple.1);
    let _ = &mut element;
}

fn shared_pair<'a>(x: &'a i32, f: impl FnOnce() -> &'a i32) -> Option<(&'a i32, &'a i32)> {
    Some((x, f()))
}
fn closure_and_argument_loans() {
    let mut both = 1;
    let separate = shared_pair(&both, || &both);
    println!("{separate:?}");
    let borrowed = &both;
    let same = shared_pair(borrowed, move || borrowed);
    println!("{same:?}");
    let _ = &mut both;
}

struct NestedSlot<'a> { inner: (&'a mut i32, bool) }
fn package_nested<'a>(f: impl FnOnce() -> &'a mut i32) -> NestedSlot<'a> {
    NestedSlot { inner: (f(), true) }
}
fn nested_closure_result_field() {
    let mut nested_item = 1;
    let mut nested = package_nested(|| &mut nested_item);
    *nested.inner.0 += 1;
    assert_eq!(nested_item, 2);
    let mut new_item = 3;
    nested = package_nested(|| &mut new_item);
    *nested.inner.0 += 2;
    assert_eq!(new_item, 5);
}
