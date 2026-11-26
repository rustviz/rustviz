// Siblings of shared_ref_of_mut_kinds_1599 found by #1619's third and
// fourth Codex reviews: each lender's loan kind is exact, and a holder
// that may point into one of several lenders says so.
fn choose<'a>(x: &'a mut String, y: &'a String, flag: bool) -> &'a String {
    if flag { x } else { y }
}

fn first<'a>(v: &Vec<&'a String>) -> &'a String {
    v[0]
}

fn fixed() -> Option<&'static String> {
    None
}

fn pick<'a>(x: &'a mut &'a String, y: &'a &'a String, f: bool) -> &'a &'a String {
    if f { x } else { y }
}

// Third Codex round: an arm lending nothing; a reference copied out of
// a holder of mixed loans, or reborrowed from a result that may hold
// either, keeps a loan of unknown kind; a copy taken before its source
// is re-pointed on the same row.
fn hedged(f: bool, g: bool) {
    let mut a18 = String::from("a");
    let b18 = String::from("b");
    let r18: &String = &mut a18;
    let o18 = if f { Some(r18) } else if g { Some(&b18) } else { fixed() };
    println!("{:?}", o18);
    let mut a19 = String::from("a");
    let b19 = String::from("b");
    let r19: &String = &mut a19;
    let x19 = vec![r19, &b19];
    let mut w19: Vec<&String> = Vec::new();
    w19.push(first(&x19));
    println!("{:?}", w19);
    let mut a20 = String::from("a");
    let b20 = String::from("b");
    let r20 = choose(&mut a20, &b20, f);
    let mut v20: Vec<&String> = Vec::new();
    v20.push(&*r20);
    println!("{:?}", v20);
    let mut a21 = String::from("a");
    let b21 = String::from("b");
    let mut r21: &String = &mut a21;
    let keep21 = r21;
    let k21 = r21; r21 = &b21;
    println!("{} {} {}", k21, keep21, r21);
}

// Fourth Codex round: `&*p` through a `&mut` is a shared reborrow; an arm
// lending only its own (`let z = &c; Some(z)`); a holder of `&&String`
// points to the reference variables; an earlier plain push keeps its
// own wording.
fn round4(f: bool, g: bool) {
    let mut x22 = String::from("x");
    let p22 = &mut x22;
    let mut v22: Vec<&String> = Vec::new();
    v22.push(&*p22);
    println!("{} {:?}", p22, v22);
    let mut a23 = String::from("a");
    let b23 = String::from("b");
    let c23 = String::from("c");
    let r23: &String = &mut a23;
    let o23 = if f { Some(r23) } else if g { Some(&b23) } else { let z23 = &c23; Some(z23) };
    println!("{:?}", o23);
    let s24 = String::from("s");
    let t24 = String::from("t");
    let mut p24 = &s24;
    let q24 = &t24;
    let mut v24: Vec<&&String> = Vec::new();
    v24.push(pick(&mut p24, &q24, f));
    println!("{:?}", v24);
    let mut a25 = String::from("a");
    let b25 = String::from("b");
    let mut v25: Vec<&String> = Vec::new();
    v25.push(&b25);
    v25.push(choose(&mut a25, &b25, f));
    println!("{:?}", v25);
}

fn main() {
    hedged(true, false);
    round4(true, false);
}
