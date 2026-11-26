struct H { f: Box<dyn FnOnce()> }
fn main() {
    let h = H { f: Box::new(|| {}) };
    (h.f)();
    let g: Box<dyn FnOnce(i32)> = Box::new(|_| {});
    let mut f: Box<dyn FnOnce(i32)> = Box::new(|_| {});
    f({ f = g; 0 });
    f(1);
}
