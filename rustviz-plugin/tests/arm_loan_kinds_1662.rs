fn fixed() -> Option<&'static String> { None }

fn arms(f: u8) {
    let mut s = String::from("s");
    let r: &String = &mut s;
    let v = match f {
        0 => Some(r),
        1 => Some(&s),
        _ => fixed(),
    };
    println!("{v:?}");
}

fn tuple(flag: bool) {
    let mut a = String::from("a");
    let r: &String = &mut a;
    let b = String::from("b");
    let c = String::from("c");
    let mut w = Vec::new();
    w.push((if flag { r } else { &b }, &c));
    println!("{b} {c} {w:?}");
}

fn deferred(flag: bool) {
    let mut d = String::from("d");
    let e = String::from("e");
    let o = Some(if flag {
        let z: &String = &mut d;
        z
    } else {
        &e
    });
    println!("{e} {o:?}");
}

fn forwarded(flag: bool) {
    let mut source = String::from("s");
    let joined: &String = if flag { &mut source } else { &source };
    let mut result = Vec::new();
    result.push(joined);
    println!("{result:?}");
}

fn deferred_same(c: bool) {
    let mut n = 4;
    let late = Some(if c {
        let left: &i32 = &mut n;
        left
    } else {
        let right = &n;
        right
    });
    println!("{late:?}");
}

fn identity(x: &i32) -> Option<&i32> { Some(x) }
fn forwarded_result(c: bool) {
    let mut x = 1;
    let r: &i32 = if c { &mut x } else { &x };
    let held = identity(r);
    println!("{held:?}");
}
