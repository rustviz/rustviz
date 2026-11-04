// Two events on one variable's column and line used to draw two dots
// at the same point, and only the top one's tooltip could be hovered
// (#309). They now share one dot whose tooltip lists both, in order.
//
// `let b = &a; let c = &a;`: a is borrowed twice on one line.
// `*r = y;`: r writes through its borrow, which then ends.
// `show(&s);`: the borrow, the call icon (`f`) and the borrow's end.
// `{ let t = String::from("t"); }`: t is created and dropped on one
// line, so the shared dot keeps the drop triangle.
fn show(s: &String) {
    println!("{}", s);
}

fn main() {
    let a = String::from("a");
    let b = &a; let c = &a;
    println!("{} {}", b, c);
    let mut x = 1;
    let y = 5;
    let r = &mut x;
    *r = y;
    println!("{}", x);
    let s = String::from("s");
    show(&s);
    { let t = String::from("t"); }
}
