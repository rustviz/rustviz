// #1340, #1341, #1352: a borrow that flows out of a call inside a
// struct, a tuple, an array or an Option, or through `?`.
struct Part<'p> { text: &'p str }
struct Two<'p> { a: &'p str, b: &'p str }
struct PartMut<'p> { text: &'p mut String }

// Callee side (#1340): the reference is copied (moved, for `&mut`) into
// the returned value; its borrow goes on in the caller.
fn part<'a>(x: &'a str) -> Part<'a> {
    Part { text: x }
}
fn pair<'a>(x: &'a str) -> (&'a str, i32) {
    (x, 1)
}
fn two<'a>(c: &'a str, d: &'a str) -> Two<'a> {
    Two { a: c, b: d }
}
fn arr<'a>(e: &'a str, f: &'a str) -> [&'a str; 2] {
    [e, f]
}
fn nested<'a>(g: &'a str) -> Option<Part<'a>> {
    Some(Part { text: g })
}
fn ok_pair<'a>(h: &'a str) -> Result<(&'a str, i32), ()> {
    Ok((h, 2))
}
fn either<'a>(k: &'a str, c: bool) -> (&'a str, i32) {
    if c {
        (k, 1)
    } else {
        (k, 2)
    }
}
fn early<'a>(m: &'a str, c: bool) -> Part<'a> {
    if c {
        return Part { text: m };
    }
    Part { text: "none" }
}
fn part_mut<'a>(y: &'a mut String) -> PartMut<'a> {
    PartMut { text: y }
}
fn pair_mut<'a>(z: &'a mut String) -> (&'a mut String, i32) {
    (z, 1)
}
fn pair_owned<'a>(x: &'a str) -> (&'a str, String) {
    (x, String::new())
}
// A reference reborrowed through the parameter goes the same way.
fn trimmed<'a>(n: &'a str) -> (&'a str, i32) {
    (n.trim(), 1)
}
fn sliced<'a>(o: &'a str) -> Part<'a> {
    Part { text: &o[1..] }
}
fn some_trim<'a>(rr: &'a str) -> Option<&'a str> {
    Some(rr.trim())
}

// Passed to a call instead: the call reads through the reference.
fn take_pair(_p: Option<(&str, i32)>) {}
fn pass_on(t: &str) {
    take_pair(Some((t, 1)));
}
// `?` on a call that borrows through the parameter, in the returned value.
fn head(hv: &Vec<String>) -> Option<&String> {
    Some(hv.first()?)
}

// Caller side (#1341, #1352): the lender stays borrowed until the
// holder's last use.
fn first_len(v: Vec<String>) -> Option<usize> {
    let x1 = v.first()?;
    println!("{}", x1);
    let w1 = v.first();
    let z1 = w1?;
    println!("{}", z1);
    Some(1)
}
fn first_res(v: &Vec<String>) -> Result<usize, &'static str> {
    let x2 = v.first().ok_or("empty")?;
    Ok(x2.len())
}
fn via_try(s5: &String) -> Option<usize> {
    let p5 = nested(s5)?;
    let q5 = ok_pair(s5).ok()?;
    println!("{} {}", p5.text, q5.0);
    Some(1)
}

fn tuple_whole() {
    let s1 = String::from("hi");
    let q1 = pair(&s1);
    println!("{}", q1.0);
    let s2 = String::from("hi");
    let q2 = pair_owned(&s2);
    println!("{}", q2.0);
}
fn through_unwrap() {
    let s3 = String::from("hi");
    let p3 = nested(&s3).unwrap();
    println!("{}", p3.text);
    let s4 = String::from("hi");
    let q4 = ok_pair(&s4).unwrap();
    println!("{}", q4.0);
}
fn tuple_mut() {
    let mut s6 = String::from("hi");
    let q6 = pair_mut(&mut s6);
    q6.0.push('!');
    println!("{}", s6);
    let mut s9 = String::new();
    let r9 = part_mut(&mut s9);
    r9.text.push('x');
}
fn reassigned() {
    let t7 = String::from("a");
    let u7 = String::from("b");
    let mut q7 = pair(&t7);
    println!("{}", q7.0);
    q7 = pair(&u7);
    println!("{}", q7.0);
}
fn literal_kinds() {
    let s8 = String::from("hi");
    let p8 = part(&s8);
    println!("{}", p8.text);
    let a8 = arr(&s8, "b");
    println!("{:?}", a8);
    let e8 = either(&s8, true);
    println!("{}", e8.0);
    let m8 = early(&s8, false);
    println!("{}", m8.text);
    let d8 = two(&s8, &s8);
    let _ = (trimmed(&s8), sliced(&s8), some_trim(&s8));
    println!("{}", d8.a);
    println!("{}", d8.b);
}
fn main() {
    tuple_whole();
    through_unwrap();
    tuple_mut();
    reassigned();
    literal_kinds();
    first_len(vec![]);
    first_res(&vec![]).ok();
    via_try(&String::from("hi"));
    pass_on("t");
    head(&vec![]);
}
