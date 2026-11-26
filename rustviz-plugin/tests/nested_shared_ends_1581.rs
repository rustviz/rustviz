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
