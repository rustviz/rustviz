struct Token;
impl Drop for Token {
    fn drop(&mut self) {}
}

fn run(c: bool) {
    let b = Box::new(Token);
    if c {
        let _t = *b;
    }
}

fn replace() {
    let mut old = Box::new(Token);
    let moved = *old;
    old = Box::new(Token);
    drop(moved);
    drop(old);
}

fn propagate(c: bool, input: Option<()>) -> Option<()> {
    let pending = Box::new(Token);
    if c {
        drop(*pending);
    }
    input?;
    Some(())
}

fn main() {
    let _ = propagate(false, None);
    run(true);
    run(false);
    replace();
}
