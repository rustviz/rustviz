fn replace_slot<'a>(dst: &mut &'a &'a i32, src: &'a &'a i32) {
    *dst = src;
}

fn field_store() {
    let a = 1;
    let b = 2;
    let r = &a;
    let q = &b;
    let mut dst = (&r, 0);
    replace_slot(&mut dst.0, &q);
    assert!(std::ptr::eq(dst.0, &q));
}

fn store_second<'a>(_: &mut &'a i32, second: &mut &'a &'a i32, src: &'a &'a i32) {
    *second = src;
}

fn second_slot() {
    let a = 1;
    let b = 2;
    let init = &a;
    let q = &b;
    let mut first = &a;
    let mut second = &init;
    store_second(&mut first, &mut second, &q);
    assert!(std::ptr::eq(first, &a));
    assert!(std::ptr::eq(second, &q));
}

fn only_return<'a>(_: &mut &'a u64, src: &'a i32) -> &'a i32 {
    src
}

fn ordinary_lender() {
    let a = 1u64;
    let b = 2;
    let mut slot = &a;
    let returned = only_return(&mut slot, &b);
    assert!(std::ptr::eq(returned, &b));
    assert!(std::ptr::eq(slot, &a));
}

fn just_return<'a>(_: &mut &'a &'a i32, src: &'a &'a i32) -> &'a &'a i32 {
    src
}

fn later_write() {
    let a = 1;
    let b = 2;
    let r = &a;
    let q = &b;
    let mut slot = &r;
    let returned = just_return(&mut slot, &q);
    assert!(std::ptr::eq(slot, &r));
    slot = returned;
    assert!(std::ptr::eq(slot, &q));
}

fn main() {
    field_store();
    second_slot();
    ordinary_lender();
    later_write();
}
