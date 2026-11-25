// #1522: a local declared in a nested arm (an else-if arm, the else after
// it) and borrowed in a conditional nested there.
fn f_1522a() {
    let s = String::from("x");
    let n = 3;
    if n == 1 {
    } else if n == 2 {
        let v = String::from("a");
        if n == 3 {
            drop(s);
        } else {
            let r = &v;
        }
    }
}
fn r_1522b(a: bool, s: String) {
    if a {
        if a {
            let mut v = String::new();
            if a {
                let m = &mut v;
            } else {
            }
            drop(s);
        }
    }
}
fn f_1522c() {
    let s = String::from("ab");
    let c0 = s.len();
    if c0 > 2 {
    } else if c0 > 1 {
        let b = String::from("ab");
        if s.len() > 1 {
            let c = &b;
            drop(s);
        }
    }
}
fn r_1522d(n: i32) {
    if n == 1 {
    } else if n == 2 {
        let s5 = String::from("ab");
        if n == 3 {
        } else {
            let r2 = &s5;
        }
    } else {
    }
}
fn f_1522e() {
    let n = 3;
    if n == 1 {
    } else if n == 2 {
    } else {
        let mut v = String::from("a");
        if n == 3 {
        } else {
            let m = &mut v;
        }
    }
}
struct Pair { a: String, b: String }
fn r_1522f(s4: String, flag: bool) {
    if flag {
    } else if flag {
        let p = Pair { a: String::new(), b: s4 };
        if flag { let r = &p.b; } else { }
    }
}
fn f_1522g() {
    let n = 3;
    if n == 1 {
    } else if n == 2 {
    } else {
        let s3 = String::new();
        if n == 4 { } else { let m6 = &s3; }
    }
}
fn r_s1522_letelse(n: i32, o: Option<String>) {
    if n == 1 {
    } else if n == 2 {
        let mut s4 = String::new();
        let Some(_x) = o else {
            let m = &mut s4;
            return;
        };
    } else {
    }
}
fn f_s1522_match() {
    let n = 3;
    if n == 1 {
    } else if n == 2 {
    } else {
        let v = String::from("a");
        match n {
            3 => {}
            _ => { let r = &v; }
        }
    }
}
fn main() {}
