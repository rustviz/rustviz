// #551: copying a `&T` / moving a `&mut T` copies / moves the reference,
// not what it points to. The pointee is a "value" when it doesn't need
// drop (`i32`, `str`), a "resource" when it does (`String`).
fn main() {
    let x = 5;
    let r1 = &x;
    let r2 = r1;
    let word = "hi";
    let w1 = &word[..];
    let w2 = w1;
    let s = String::from("hello");
    let q1 = &s;
    let q2 = q1;
    let mut n = 1;
    let m1 = &mut n;
    let m2 = m1;
    *m2 += 1;
    let mut t = String::from("a");
    let u1 = &mut t;
    let u2 = u1;
    u2.push('b');
    println!("{} {} {} {} {} {}", r1, r2, w1, w2, q1, q2);
}
