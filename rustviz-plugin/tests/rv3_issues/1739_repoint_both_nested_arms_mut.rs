fn f(a: &mut String, b: &mut String, n: i32, d: bool) {
    let mut p = &mut *a;
    match n {
        1 => {
            if d {
                p = &mut *b;
            } else {
                p = &mut *b;
            }
            let z = &*p;
            a.push('x');
            z.len();
        }
        _ => {
            p.len();
        }
    }
}
fn main() { let mut a = String::new(); let mut b = String::new(); f(&mut a, &mut b, 1, true); }
