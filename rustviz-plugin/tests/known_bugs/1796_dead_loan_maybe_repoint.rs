// Known bug, from the closed 2.x fix PRs (cx/cx-text-b); kept as a RustViz 3 regression case.
// Issue(s): #1796 (see known_bugs/README.md for what main draws wrong).
fn push(mut x: String, y: String, c: bool, d: bool) {
    let mut r = &x;
    if c {
        x.push('x');
        if d {
            r = &y;
            println!("{}", r);
        }
    } else {
        println!("{}", r);
    }
}

fn assign(mut a: String, b: String, c: bool, d: bool) {
    let mut q = &a;
    match c {
        true => {
            a = String::new();
            let true = d else { return };
            q = &b;
            println!("{}", q);
        }
        false => println!("{}", q),
    }
}
