// #1153: a reference given a borrow in each branch of an `if` (`let r;`).
// The note "r's borrow of s is already over here" is for a write where
// r's earlier loan is dead on that path; on the `else` path r never held
// the `if` branch's loan, so the note on the else's own borrow (or on a
// field write there) was false. It still appears where it is true
// (`still_noted`).
fn main() {
    let mut s = String::from("a");
    let n = 2;
    let r: &mut String;
    if n > 1 {
        r = &mut s;
    } else {
        r = &mut s;
    }
    r.push('c');
}

struct P { n: i32 }

fn field() {
    let mut p = P { n: 1 };
    let c = true;
    let q: &P;
    if c {
        q = &p;
    } else {
        p.n = 5;
        q = &p;
    }
    println!("{}", q.n);
}

fn arms() {
    let mut t = String::from("t");
    let k = 2;
    let w: &mut String;
    match k {
        1 => w = &mut t,
        _ => {
            w = &mut t;
        }
    }
    w.push('d');
}

// The write is on a path where x's loan really is dead (#796).
fn still_noted() {
    let mut x = 1;
    let y = 2;
    let mut z = &x;
    let cond = true;
    if cond {
        x = 2;
        z = &y;
    }
    println!("{} {}", z, x);
}
