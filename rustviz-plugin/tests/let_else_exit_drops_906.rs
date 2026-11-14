// #906: the `else` block of a let-else runs when the pattern doesn't
// match, and has to leave (`return`, `panic!`, `continue`, ..). What's
// still owned there is dropped on that path, the scrutinee too (it was
// not moved: the pattern didn't match), as at the `return` in
// `if opt.is_none() { return; }`. A struct or Box drawn with part
// columns is dropped whole there: its part isn't moved yet (also after
// an `if c { return; }`, `if_return`).
struct P {
    a: Option<String>,
    b: String,
}

fn one_line(opt: Option<String>) {
    let s = String::from("x");
    let Some(t) = opt else { return; };
    println!("{}", t);
    drop(s);
}

fn multi_line(r: Result<String, String>) {
    let k = String::from("k");
    let Ok(n) = r else {
        let msg = String::from("bad");
        panic!("{}", msg);
    };
    println!("{}", n);
    drop(k);
}

fn parts(p: P) {
    let P { a: Some(a), .. } = p else { return; };
    println!("{}", a);
}

fn boxed(b: Box<(Option<String>, i32)>) {
    let (Some(s2), n2) = *b else { return; };
    println!("{} {}", s2, n2);
}

fn if_return(k: i32) {
    let q = P { a: Some(String::from("a")), b: String::from("b") };
    if k == 0 {
        return;
    }
    let qa = q.a;
    println!("{:?}", qa);
}

fn looped(v: Vec<Option<String>>) {
    for o in v {
        let w = String::from("w");
        let Some(x) = o else { continue; };
        println!("{} {}", w, x);
    }
}

fn main() {
    one_line(Some(String::from("y")));
    multi_line(Ok(String::from("n")));
    parts(P { a: None, b: String::from("b") });
    boxed(Box::new((None, 2)));
    if_return(0);
    looped(vec![None]);
}
