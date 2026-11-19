// #1249: a function with two `&mut` parameters of one type (a type
// parameter, or `&'a String` with one named lifetime) may exchange or copy
// the references r1 and r2 hold: afterwards each may point to either
// lender, so each gives both back. Distinct lifetimes (`separate`) can't
// exchange them; `mem::swap` is drawn exactly (#1230).
fn exchange<T>(x: &mut T, y: &mut T) { std::mem::swap(x, y); }
fn copy_over<'a>(x: &mut &'a String, y: &mut &'a String) { *x = *y; }
fn separate(x: &mut &String, y: &mut &String) { let _ = (x, y); }

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r1 = &a;
    let mut r2 = &b;
    exchange(&mut r1, &mut r2);
    println!("{}", r1);
}

fn named() {
    let c = String::from("c");
    let d = String::from("d");
    let mut r3 = &c;
    let mut r4 = &d;
    copy_over(&mut r3, &mut r4);
    println!("{} {}", r3, r4);
}

fn distinct() {
    let e = String::from("e");
    let f = String::from("f");
    let mut r5 = &e;
    let mut r6 = &f;
    separate(&mut r5, &mut r6);
    println!("{} {}", r5, r6);
}

fn swapped() {
    let g = String::from("g");
    let h = String::from("h");
    let mut r7 = &g;
    let mut r8 = &h;
    std::mem::swap(&mut r7, &mut r8);
    println!("{}", r7);
}
