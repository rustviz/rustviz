// Known bug, from the closed 2.x fix PRs (fix/r3-repoint); kept as a RustViz 3 regression case.
// Issue(s): #1575 (see known_bugs/README.md for what main draws wrong).
// A dead first loan of a re-pointed reference (`let mut r2 = &s;` never
// read, later `r2 = &t;`) doesn't keep a later `&mut s` from giving s
// back: m3's borrow ends at its last use, with a return arrow (#1575). It
// used to end with m3's scope, giving nothing back, and s stayed mutably
// borrowed to its own scope end.

fn take_gen<T>(_t: T) {}

fn in_an_if(b: bool) {
    let mut s = String::from("hi");
    let mut r2 = &s;
    if b {
        let m3 = &mut s;
        m3.push('a');
    }
    let t = String::new();
    r2 = &t;
}

fn in_a_loop() {
    let mut s = String::from("hi");
    let mut r2 = &s;
    loop {
        let m3 = &mut s;
        m3.push('a');
        break;
    }
    let t = String::new();
    r2 = &t;
}

fn moved_into_a_call() {
    let mut s = String::from("hi");
    let mut r2 = &s;
    loop {
        let m3 = &mut s;
        take_gen(m3);
        break;
    }
    let t = String::new();
    r2 = &t;
}

fn main() {}
