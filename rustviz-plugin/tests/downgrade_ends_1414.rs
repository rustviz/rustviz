// #1414: `downgrade(&mut s) -> &String` holds a mutable loan of s for as
// long as its result is live, wherever the result goes (a struct field, a
// tuple, a reference declared in an arm, a reassigned reference). The loan
// ends before s is used again, also on the same line (`println!("{} {}",
// h.n, s)` reads h.n, then s), and gives s back.
struct Holder<'a> {
    r: &'a String,
    n: i32,
}

struct HolderMut<'a> {
    r: &'a mut String,
    n: i32,
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn literal_same_line() {
    let mut s = String::from("x");
    let h = Holder { r: downgrade(&mut s), n: 1 };
    println!("{} {}", h.n, s);
}

fn mut_field_same_line() {
    let mut s = String::from("x");
    let h = HolderMut { r: &mut s, n: 1 };
    println!("{} {}", h.n, s);
}

fn plain_same_line() {
    let mut s = String::from("x");
    let r = &mut s;
    println!("{} {}", r.len(), s);
}

fn tuple() {
    let mut s = String::from("x");
    let t = (downgrade(&mut s), 1);
    println!("{} {}", t.1, s);
}

fn in_arm(flag: bool) {
    let mut s = String::from("ab");
    let x2 = first(&s);
    if flag {
        let r2 = downgrade(&mut s);
        println!("{}", r2);
    } else {
        println!("{}", x2);
    }
    println!("{}", s);
}

fn reassigned() {
    let mut s2 = String::from("ab");
    let vr = vec![s2.as_str(), "hi"];
    println!("{:?}", vr);
    let mut r = downgrade(&mut s2);
    let n = s2.len();
    if n > 1 {
        r = downgrade(&mut s2);
        println!("{}", r);
    }
}

fn other_path(flag: bool) {
    let mut s3 = String::from("ab");
    let h = Holder { r: &s3, n: 0 };
    if flag {
        let r2 = downgrade(&mut s3);
        let n = r2.len() as i32;
        let h2 = Holder { r: &s3, n };
        println!("{}", h2.n);
    } else {
        println!("{}", h.r);
    }
    println!("{}", s3);
}

// A mutable loan in one arm, a shared one made before the `if` and used
// only in the other arm: on the `if` path the shared one is over, and
// the mutable one gives o back.
fn as_mut_in_one_arm(n: i32) {
    let mut o = Some(String::from("a"));
    let q = o.as_ref().unwrap();
    if n > 0 {
        let m = o.as_mut().unwrap();
        m.push('!');
    } else {
        let q2 = q;
        println!("{}", q2);
        o.take();
    }
    println!("{:?}", o);
}

fn main() {
    as_mut_in_one_arm(1);
    literal_same_line();
    mut_field_same_line();
    plain_same_line();
    tuple();
    in_arm(true);
    reassigned();
    other_path(true);
}
