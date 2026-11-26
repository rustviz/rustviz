// Known bug, from the closed 2.x fix PRs (cx/cx-c04); kept as a RustViz 3 regression case.
// Issue(s): #1581 (see known_bugs/README.md for what main draws wrong).
fn run(a: bool, b: bool) {
    let s = String::new();
    let r = &s;
    let r2 = &s;
    if a {
    } else if b {
    } else {
        let n = r2.len();
    }
    println!("{}", r);
}

fn nested_let_else(r: &String, m: &mut String, o2: Option<String>, k: u8) {
    match k {
        0 => {
            let Some(x2) = o2 else {
                let t = std::mem::replace(m, String::new());
                return;
            };
        }
        _ => {}
    }
}
