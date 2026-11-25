// A reference with two lenders (`r = if c { &a } else { &b };`) re-pointed
// while another reference `x = &b` still borrows b: r's old loan ends, a is
// given back, but b stays borrowed by x until x's last use, which gives it
// back (#1493). It used to give b back at r's end too, and again at x's.

fn straight(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let t = String::from("t");
    let mut r = if c { &a } else { &b };
    let x = &b;
    println!("{}", r);
    r = &t;
    println!("{}", r);
    println!("{}", x);
}

// The re-point in one arm, r read in the other.
fn in_arms(c: bool) {
    let a2 = String::from("a");
    let b2 = String::from("b");
    let t2 = String::from("t");
    let mut r2 = if c { &a2 } else { &b2 };
    let x2 = &b2;
    if c {
        r2 = &t2;
        println!("{}", r2);
    } else {
        println!("{}", r2);
    }
    println!("{}", x2);
}

fn main() {
    straight(true);
    in_arms(false);
}
