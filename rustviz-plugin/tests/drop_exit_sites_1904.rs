struct Guard(bool);

impl Drop for Guard {
    fn drop(&mut self) {
        if self.0 {
            panic!("during replacement");
        }
    }
}

pub fn intervening_drop(consume: bool) {
    let replacement = String::from("new");
    let mut x = String::from("old");
    let mut guard = Guard(true);
    if consume {
        drop(x);
    }
    guard = Guard(false);
    x = replacement;
    drop((x, guard));
}

pub fn loop_write(consume: bool) {
    let replacement = String::from("new");
    let mut x = String::from("old");
    if consume {
        drop(x);
    }
    loop {
        x = replacement;
        break;
    }
    drop(x);
}

pub fn may_exit(consume: bool, quit: bool) {
    let replacement = String::from("new");
    let mut x = String::from("old");
    if consume {
        drop(x);
    }
    if quit {
        std::process::exit(0);
    }
    x = replacement;
    drop(x);
}

extern "C" fn fail() {
    panic!("cannot unwind");
}

pub fn may_terminate(consume: bool) {
    let replacement = String::from("new");
    let mut x = String::from("old");
    if consume {
        drop(x);
    }
    x = { fail(); replacement };
    drop(x);
}

pub fn residual_at_brace(c: bool, o: Option<()>) -> Option<()> {
    let s;
    if c {
        s = String::from("old");
    }
    o?; Some(()) }

pub fn two_residuals(c: bool, first: Option<()>, second: Option<()>) -> Option<()> {
    let mut s;
    let replacement = String::from("new");
    if c {
        s = String::from("old");
    }
    first?; s = replacement; second?;
    Some(())
}

enum E {
    Both(String, String),
}

pub struct Wrapper {
    e: E,
}

pub fn enum_payload(w: Wrapper) {
    let E::Both(s, _) = w.e;
    drop(s);
}

pub fn boxed(c: bool, p: Box<(Option<String>, String)>) {
    let Some(s) = p.0 else { return };
    drop(s);
    if c {
        drop(p.1);
    }
}

pub struct Pair {
    a: String,
    b: String,
}

pub fn same_row(c: bool, p: Pair) {
    if c { drop(p.a); return; } else { return; }
}

mod private {
    pub struct P {
        pub taken: String,
        hidden: u8,
    }
}

pub fn no_remaining_drop(p: private::P) {
    drop(p.taken);
}
