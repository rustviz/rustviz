// #873: a constructor is never an actor, whatever its type. A
// constructor of a non-Copy type builds a new value in place: no column
// for the constructor and no arrow out of it. A call that makes the
// payload keeps its arrow, and a variable moved in keeps its move.
#[derive(Debug)]
enum Fruit { Apple(u32), Banana }
#[derive(Debug)]
enum Pair { Two(u32, u32) }
struct Marker;
struct Wrapper(String);
#[derive(Clone, Copy)]
struct Pt(i32, i32);

fn main() {
    let mut y = Fruit::Banana;
    let p = Pair::Two(1, 2);
    let c = 3;
    y = Fruit::Apple(c + 1);
    let a = String::from("a");
    let b = String::from("b");
    let mut o = Some(a + &b);
    let o2 = Some(format!("x"));
    o = Some(format!("y"));
    let w = Some(String::from("w"));
    let s = String::from("s");
    let m = Some(s);
    let n: Option<String> = None;
    let k = Marker;
    let t = Wrapper(format!("t"));
    let pt = Pt(1, 2);
    let px = pt.0;
    println!("{:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {}", y, p, o, o2, w, m, n, t.0, px);
}
