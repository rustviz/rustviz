// #1277: a reference declared in a one-line arm that has a row of its
// own (#1187, #1264) ends its borrow on that arm's row, with its last
// use and before its scope end, not on the join row below, where it no
// longer exists. The lender's column says it is still borrowed by the
// reference the conditional's other arm could use (`ra`), whose end is
// on the join row.
fn stacked(c: bool) {
    let a = String::from("a");
    let f = String::from("f");
    let g = String::from("g");
    let h = String::from("h");
    let ra = &a;
    let (s, t) = if c { (f, g) } else { let x = ra; (x.to_string(), h) };
    println!("{} {}", s, t);
}

fn overlap(c: bool) {
    let b = String::from("b");
    let rb = &b;
    let f = String::from("f");
    let h = String::from("h");
    let s = if c { f } else { let y = rb; println!("{}", y); h };
    println!("{}", s);
}

fn copied_on(c: bool) {
    let d = String::from("d");
    let rd = &d;
    let f = String::from("f");
    let h = String::from("h");
    let s = if c { f } else { let w = rd; let z = w; println!("{}", z); h };
    println!("{}", s);
}

fn in_loop(v: Vec<(String, String)>, c: bool) {
    let e = String::from("e");
    let re = &e;
    for (f, h) in v {
        let s = if c { f } else { let q = re; println!("{}", q); h };
        println!("{}", s);
    }
}

// The lender is used in the conditional: the plugin nests the end in
// the arm already; it goes before the scope end in the arm's dot.
fn lender_in_arm(n: u8) {
    let mut m = String::from("m");
    let f = String::from("f");
    let h = String::from("h");
    let s = match n { 0 => f, _ => { let p = &mut m; p.push('!'); h } };
    println!("{} {}", s, m);
}

fn main() {
    stacked(true);
    overlap(true);
    copied_on(true);
    in_loop(Vec::new(), true);
    lender_in_arm(1);
}
