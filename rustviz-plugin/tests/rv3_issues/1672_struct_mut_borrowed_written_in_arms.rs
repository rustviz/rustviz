struct S { n: i32, a: String }
fn main() {
    let mut s = S { n: 1, a: String::new() };
    let flag = s.n > 0;
    let r1 = &mut s;
    if flag {
        r1.n = 3;
        r1.a.push('x');
    } else {
        r1.n = 4;
    }
    println!("{}", s.n);
}
