// #1441: in a one-line guarded match arm, the value paired with the `let`
// after the arm is visited is drawn moved / borrowed a row above its
// acquire (event-on-dead-column).
fn deferred_var(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(n) if n > 0 => { let y = String::from("y"); y }
        _ => s,
    };
    println!("{}", r);
}

fn branch_temp(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(n) if n > 0 => &String::from("pos"),
        _ => &s,
    };
    println!("{}", r);
}

fn main() {
    deferred_var(Some(1));
    branch_temp(Some(1));
}
