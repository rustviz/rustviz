// r borrows from either a or b. Both loans end at r's last read
// before `r = &c;`: `a += 1;` and `b += 1;` compile (a write in the
// `println!` argument, `{ a += 1; r }`, is E0506) (#770).
fn main() {
    let mut a = 1;
    let mut b = 2;
    let c = 3;
    let flag = a > 0;
    let mut r = if flag { &a } else { &b };
    println!("{}", r);
    a += 1;
    b += 1;
    r = &c;
    println!("{} {} {}", r, a, b);
}
