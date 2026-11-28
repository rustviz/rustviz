use std::cell::RefCell;
use std::rc::Rc;

fn take(_: &str) {}

fn coercion() {
    let s = String::from("hello");
    take(&s);
}

fn pattern_guard(value: Option<String>) {
    match value {
        Some(ref x) if x.is_empty() => take(x),
        _ => {}
    }
}

fn cast_chain() {
    let x = 42i32;
    let p = &x as *const i32 as *const u8;
    let mut y = 42u32;
    let q = &mut y as *mut u32 as *mut u8;
    let _ = (p, q);
}

fn nested_index(slots: Vec<&usize>) -> usize {
    let value = *slots[
        *slots[0]
    ];
    value
}

fn raw_scope(x: bool) -> bool {
    let p = &x as *const bool;
    // The dereference is evaluated, but its use has no separate dot.
    unsafe { *p && x }}


fn raw_use() {
    let x = 42;
    { let p = &x as *const i32;
      let value = unsafe { *p }; }
}

fn guarded() {
    let shared = Rc::new(RefCell::new(1));
    let mut guard = shared.borrow_mut();
    *guard += 1;
}

struct Job;
impl Job {
    async fn run(&self) {}
}

fn future(job: Job) {
    let fut = job.run();
    let _ = fut;
}

macro_rules! make_future {
    () => { async { 123 } };
}

fn generated() {
    let fut = make_future!();
    let _ = fut;
}

fn nested_iter(outer: Vec<Vec<String>>) {
    for group in outer {
        for item in group {
            drop(item);
        }
    }
}

struct Spy(&'static str);
impl Drop for Spy {
    fn drop(&mut self) { println!("{}", self.0); }
}

fn reassigned(make: bool) {
    let other = Spy("other");
    let mut r = if make {
        &Spy("extended")
    } else { &other };
    println!("{}", r.0);
    r = &other;
    println!("{}", r.0);
}

fn destructured(make: bool) {
    let other = Spy("other");
    let (r,) = (if make {
        &Spy("tuple")
    } else { &other },);
    println!("{}", r.0);
}
