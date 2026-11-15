// #997: after `keep(&mut v, &s)` may store `&s` in v (#513), the
// borrow is held by whatever holds v's value, each while it is in use
// (rustc's loan liveness):
// * v is Copy (`Option<&String>`): `let w = v;` copies the borrow, so
//   both v and w hold it, each until its own last use. Main named v at
//   `look(&s)` although only w is used after it.
// * v is moved in one arm (`if c { let x = u; .. } else { .. }`): the
//   borrow goes on to x on that arm's path only, and u keeps it on the
//   other. Main said "t's resource is no longer immutably borrowed" in
//   both arms; rustc rejects moving t right after either `look(&t)`.
// * v isn't used on one arm's path: it doesn't hold the borrow there.
fn keep<'a>(hold: &mut Option<&'a String>, r: &'a String) {
    *hold = Some(r);
}

fn keepv<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn look(r: &String) -> usize {
    r.len()
}

fn main() {
    let s = String::from("s");
    let mut v = None;
    keep(&mut v, &s);
    let w = v;
    let n = look(&s);
    println!("{:?} {}", w, n);
}

// Both v and its copy in use; a copy of a copy.
fn copies() {
    let a = String::from("a");
    let mut va = None;
    keep(&mut va, &a);
    let wa = va;
    let na = look(&a);
    println!("{:?}", wa);
    let ma = look(&a);
    println!("{:?} {} {}", va, na, ma);

    let b = String::from("b");
    let mut vb = None;
    keep(&mut vb, &b);
    let wb = vb;
    let xb = wb;
    let nb = look(&b);
    println!("{:?} {}", xb, nb);
}

fn branch(c: bool) {
    let t = String::from("t");
    let mut u = Vec::new();
    keepv(&mut u, &t);
    if c {
        let x = u;
        let n = look(&t);
        println!("{} {}", x.len(), n);
    } else {
        let n = look(&t);
        println!("{} {}", u.len(), n);
    }
}

// Moves in different arms: only one of them happens.
fn arms(k: u8) {
    let d = String::from("d");
    let mut vd = Vec::new();
    keepv(&mut vd, &d);
    match k {
        0 => {
            let wd = vd;
            let n0 = look(&d);
            println!("{} {}", wd.len(), n0);
        }
        1 => {
            let xd = vd;
            let n1 = look(&d);
            println!("{} {}", xd.len(), n1);
        }
        _ => {
            let n2 = look(&d);
            println!("{} {}", vd.len(), n2);
        }
    }
}

// A copy in one arm; the original is used only in the other.
fn copy_in_arm(c: bool) {
    let e = String::from("e");
    let mut ve = None;
    keep(&mut ve, &e);
    if c {
        let we = ve;
        let ne = look(&e);
        println!("{:?} {}", we, ne);
    } else {
        let ne = look(&e);
        println!("{:?} {}", ve, ne);
    }
}

// The holder is used only in the else arm: in the if arm f is free.
fn unused_in_arm(c: bool) {
    let f = String::from("f");
    let mut vf = Vec::new();
    keepv(&mut vf, &f);
    if c {
        let nf = look(&f);
        println!("{}", nf);
    } else {
        let nf = look(&f);
        println!("{} {}", vf.len(), nf);
    }
}
