// #1373: a one-row match arm is solid over its own row (#845), with or
// without an event on the column there. main drew a middle arm with an
// event (`1 => show(&s),`) dashed from the fork to the join, straight
// through its own dot.
fn consume(_s: String) {}
fn show(_s: &String) {}
fn owner(n: u8) {
    let s = String::from("hi");
    match n {
        0 => consume(s),
        1 => show(&s),
        _ => show(&s),
    }
}
fn five(n: u8) {
    let s = String::from("hi");
    match n {
        0 => show(&s),
        1 => show(&s),
        2 => {}
        3 => show(&s),
        _ => consume(s),
    }
}
// A middle arm that moves it: solid above its dot only.
fn middle_move(n: u8) {
    let s = String::from("hi");
    match n {
        0 => show(&s),
        1 => consume(s),
        _ => show(&s),
    }
}
// A Copy value read in a middle arm.
fn copy_value(n: u8) {
    let x = 5;
    let mut y = 0;
    match n {
        0 => y = x,
        1 => y = x + 1,
        _ => {}
    }
    println!("{} {}", x, y);
}
fn main() {
    owner(1);
    five(2);
    middle_move(1);
    copy_value(1);
}
