// Forming an address, borrowing the wrapper, and joining paths do not
// replace its contents. A write of a fresh wrapper does.
use std::hint::black_box;
use std::mem::ManuallyDrop;

fn after_join(flag: bool) {
    let mut slot = ManuallyDrop::new(String::from("old"));
    let old = unsafe { ManuallyDrop::take(&mut slot) };
    if flag {
        black_box(flag);
    }
    let addr = &raw const slot;
    let r = &slot;
    black_box(r);
    black_box(addr);
    drop(old);
}

fn maybe_replaced(flag: bool) {
    let mut maybe_slot = ManuallyDrop::new(String::from("old"));
    let old = unsafe { ManuallyDrop::take(&mut maybe_slot) };
    if flag {
        maybe_slot = ManuallyDrop::new(String::new());
    }
    drop(old);
    maybe_slot = ManuallyDrop::new(String::from("second replacement"));
    drop(ManuallyDrop::into_inner(maybe_slot));
}

fn through_raw_pointer() {
    let mut raw_slot = ManuallyDrop::new(String::from("old"));
    let old = unsafe { ManuallyDrop::take(&mut raw_slot) };
    let dst = &raw mut raw_slot;
    unsafe { std::ptr::write(dst, ManuallyDrop::new(String::from("new"))); }
    drop(old);
    drop(ManuallyDrop::into_inner(raw_slot));
}

fn function_item() {
    let mut alias_slot = ManuallyDrop::new(String::from("old"));
    let take = ManuallyDrop::<String>::take;
    let old = unsafe { take(&mut alias_slot) };
    drop(old);
}

fn copyable_wrapper() {
    let mut number = ManuallyDrop::new(7_u32);
    let n = unsafe { ManuallyDrop::take({ &mut number }) };
    black_box(n);
}

fn anonymous_type() {
    let text = String::from("captured");
    let mut closure_slot = ManuallyDrop::new(move || drop(text));
    let closure = unsafe { ManuallyDrop::take(&mut closure_slot) };
    closure();
}
