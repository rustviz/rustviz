struct P<F: FnOnce() -> String> { f: F, z: String }
fn run<F: FnOnce() -> String>(p: P<F>, c: bool) {
    if c {
        let t = (p.f)();
        drop(t);
    }
}
