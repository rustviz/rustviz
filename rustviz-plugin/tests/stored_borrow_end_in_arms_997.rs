// #997: a borrow a call stored (`keepv(&mut u, &t)`, #513) whose loan
// ends inside an arm of an `if` / `match` (the last holder's last use on
// one path) ends on every path: in each arm after the last line a holder
// holds it on that arm's path, or where the arm starts if none does.
// Main drew the end on the lender's column outside the arms, in the
// middle of the conditional, and the join drew that column twice over.
fn keepv<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn keep_mut<'a>(hold: &mut Vec<&'a mut String>, r: &'a mut String) {
    hold.push(r);
}

fn look(r: &String) -> usize {
    r.len()
}

fn main() {
    no_else(true);
    else_if(1);
    mutable(true);
    in_arm(true);
    chain_in_arm(true);
}

// No else: on the path that skips the `if`, v is dead.
fn no_else(c: bool) {
    let s = String::from("s");
    let mut v = Vec::new();
    keepv(&mut v, &s);
    if c {
        let n = look(&s);
        println!("{} {}", v.len(), n);
    }
    let s2 = s;
    println!("{}", s2);
}

// The borrow moves on to x in one arm, stays with u in another, and
// nothing holds it in the last one.
fn else_if(k: u8) {
    let t = String::from("t");
    let mut u = Vec::new();
    keepv(&mut u, &t);
    if k == 0 {
        let x = u;
        println!("{}", x.len());
    } else if k == 1 {
        let n = look(&t);
        println!("{} {}", u.len(), n);
    } else {
        let n = look(&t);
        println!("{}", n);
    }
}

// A `&mut` borrow: m is usable again in each arm after its holder's
// last use there.
fn mutable(c: bool) {
    let mut m = String::from("m");
    let mut w = Vec::new();
    keep_mut(&mut w, &mut m);
    if c {
        let y = w;
        println!("{}", y.len());
        m.push('!');
    } else {
        println!("{}", w.len());
        m.push('?');
    }
}

// Stored inside an arm, ending in a nested conditional.
fn in_arm(c: bool) {
    let a = String::from("a");
    let mut va = Vec::new();
    if c {
        keepv(&mut va, &a);
        if va.len() > 3 {
            let n = look(&a);
            println!("{} {}", va.len(), n);
        } else {
            let n = look(&a);
            println!("{}", n);
        }
    }
}

// Two moves one after the other in one arm (not alternatives).
fn chain_in_arm(c: bool) {
    let b = String::from("b");
    let mut vb = Vec::new();
    keepv(&mut vb, &b);
    if c {
        let wb = vb;
        let yb = wb;
        let n = look(&b);
        println!("{} {}", yb.len(), n);
    }
}
