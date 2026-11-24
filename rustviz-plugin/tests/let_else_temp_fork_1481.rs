struct W { v: Vec<String> }
impl W { fn first(&self) -> Option<(String, String)> { None } }
fn opt_pair() -> Option<(String, String)> { Some((String::from("a"), String::from("b"))) }
fn res() -> Result<String, String> { Ok(String::from("ok")) }

// Multi-line, drawn as a branch (s is dropped by the `return`).
fn result_multi() {
    let s = String::from("s");
    let Ok(v) = res() else {
        return;
    };
    println!("{} {}", s, v);
}

// One line, drawn as a branch.
fn one_line_branch() {
    let s = String::from("s");
    let Some((b, _)) = opt_pair() else { return; };
    println!("{} {}", s, b);
}

// A method call's result.
fn method_call(w: W) {
    let Some((b, _)) = w.first() else {
        return;
    };
    println!("{}", b);
}

// In a loop, leaving with `continue`.
fn in_loop() {
    let s = String::from("s");
    for _ in 0..2 {
        let t = String::from("t");
        let Some((b, _)) = opt_pair() else {
            continue;
        };
        println!("{} {} {}", s, t, b);
    }
}

// Inside an `if` arm.
fn in_arm(c: bool) {
    let s = String::from("s");
    if c {
        let Some((b, _)) = opt_pair() else {
            return;
        };
        println!("{}", b);
    }
    println!("{}", s);
}

// A `ref` binding borrows from the temporary.
fn ref_binding() {
    let s = String::from("s");
    let Some((ref b, _)) = opt_pair() else {
        return;
    };
    println!("{} {}", s, b);
}

fn main() {}
