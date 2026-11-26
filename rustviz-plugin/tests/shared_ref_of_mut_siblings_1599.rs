// More siblings of shared_ref_of_mut_kinds_1599 (second Codex review):
// what a holder gets through `extend`, a call, a field reborrow, a
// coercion or another reference, and which arm lends what.
struct S {
    x: String,
}

struct H<'a> {
    v: Vec<&'a String>,
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn first<'a>(v: &Vec<&'a String>) -> &'a String {
    v[0]
}

fn hold(s: &String) -> H<'_> {
    H { v: vec![s] }
}

fn choose<'a>(x: &'a mut String, y: &'a String, flag: bool) -> &'a String {
    if flag { x } else { y }
}

fn id(s: &String) -> &String {
    s
}

// A real `&mut i32` stored by `extend`: v can write through it.
fn extended() {
    let mut a1 = 1;
    let mut v1: Vec<&mut i32> = Vec::new();
    v1.extend([&mut a1]);
    *v1[0] = 4;
}

// A call's shared borrow next to a `&mut`-derived `&`; a `&mut` next to
// one.
fn mixed_store() {
    let a2 = String::from("a");
    let mut b2 = String::from("b");
    let mut v2: Vec<(&str, &String)> = Vec::new();
    let r2: &String = &mut b2;
    v2.push((a2.as_str(), r2));
    println!("{}", a2);
    println!("{:?}", v2);
    let mut a3 = 1;
    let mut b3 = 2;
    let mut v3: Vec<(&i32, &mut i32)> = Vec::new();
    let r3: &i32 = &mut a3;
    v3.push((r3, &mut b3));
    *v3[0].1 = 3;
}

// The loan of what `first` copies out is the stored reference's own.
fn copied_out(flag: bool) {
    let a4 = String::from("a");
    let mut x4: Vec<&String> = vec![&a4];
    let mut w4: Vec<&String> = Vec::new();
    w4.push(first(&mut x4));
    println!("{}", a4);
    println!("{:?}", w4);
    let mut a5 = String::from("a");
    let r5: &String = &mut a5;
    let x5: Vec<&String> = vec![r5];
    let rx5 = &x5;
    let mut w5: Vec<&String> = Vec::new();
    w5.push(first(rx5));
    println!("{:?}", w5);
}

// A `&String` made from `&mut` by a call or a coercion, a field
// reborrowed through a `&mut`-derived `&`, a branch's call.
fn made_shared(flag: bool) {
    let mut a6 = String::from("a");
    let mut v6: Vec<&String> = Vec::new();
    v6.push(downgrade(&mut a6));
    println!("{:?}", v6);
    let mut a7 = String::from("a");
    let mut v7: Vec<&String> = Vec::new();
    v7.push(&mut a7);
    println!("{:?}", v7);
    let mut a8 = S { x: String::from("a") };
    let r8: &S = &mut a8;
    let mut v8: Vec<&String> = Vec::new();
    v8.push(&r8.x);
    println!("{:?}", v8);
    let mut a9 = String::from("a");
    let e9 = String::from("e");
    let o9 = Some(if flag { downgrade(&mut a9) } else { &e9 });
    println!("{:?}", o9);
}

// Through another reference; a struct field holding references; each
// arm's own lender.
fn through_refs(flag: bool) {
    let mut a10 = String::from("a");
    let b10 = String::from("b");
    let p10 = &mut a10;
    let r10 = downgrade(p10);
    let o10 = Some(if flag { r10 } else { &b10 });
    println!("{:?}", o10);
    let mut a11 = String::from("a");
    let r11: &String = &mut a11;
    let h11 = hold(r11);
    println!("{:?}", h11.v);
    let mut a12 = String::from("a");
    let b12 = String::from("b");
    let r12: &String = &mut a12;
    let o12 = if flag {
        Some(r12)
    } else {
        Some(&b12)
    };
    println!("{:?}", o12);
}

// Each argument's own kind; a `&mut` coerced into a `&String` parameter;
// an element reborrowed through a `&mut`-derived `&`; a copy of a
// reference re-pointed on its binding row.
fn per_argument(flag: bool) {
    let mut a13 = String::from("a");
    let b13 = String::from("b");
    let mut v13: Vec<&String> = Vec::new();
    v13.push(choose(&mut a13, &b13, flag));
    println!("{} {:?}", b13, v13);
    let a14 = String::from("a");
    let mut b14 = String::from("b");
    let mut v14: Vec<(&String, &mut String)> = Vec::new();
    v14.extend([(&a14, &mut b14)]);
    println!("{}", a14);
    v14[0].1.push('x');
    let mut a15 = String::from("a");
    let mut v15: Vec<&String> = Vec::new();
    v15.push(id(&mut a15));
    println!("{:?}", v15);
    let mut a16 = [1, 2];
    let r16: &[i32; 2] = &mut a16;
    let mut v16: Vec<&i32> = Vec::new();
    v16.push(&r16[0]);
    println!("{:?}", v16);
    let mut a17 = String::from("a");
    let b17 = String::from("b");
    let mut r17: &String = &mut a17; r17 = &b17;
    let k17 = r17;
    println!("{} {}", k17, a17);
}

fn main() {
    per_argument(true);
    extended();
    mixed_store();
    copied_out(true);
    made_shared(true);
    through_refs(true);
}
