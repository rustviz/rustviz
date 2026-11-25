// A `&mut` declared in an arm (`let m2 = &mut s;`) ends at its last use and
// gives s back; a later reference re-pointed to s (`r3 = &s;`) doesn't take
// m2's loan over: m2 isn't a copy its old value carried (#1515). The arm's
// loan used to be kept, so s was never given back and its column ended.

fn in_if() {
    let mut s = String::from("ab");
    if s.len() > 1 {
        let m2 = &mut s;
        m2.push('x');
    }
    let mut r3 = &s;
    r3 = &s;
}

// In the `else` arm.
fn in_else() {
    let mut s2 = String::from("ab");
    if s2.len() > 1 {
        println!("short");
    } else {
        let m4 = &mut s2;
        m4.push('x');
    }
    let mut r5 = &s2;
    r5 = &s2;
}

// In a match arm, never used; re-pointed in a later `if`.
fn in_match() {
    let mut s3 = String::from("ab");
    match s3.len() {
        0 => {
            let _m6 = &mut s3;
        }
        _ => {}
    }
    let mut r7 = &s3;
    if s3.is_empty() {
        r7 = &s3;
    }
}

fn main() {
    in_if();
    in_else();
    in_match();
}
