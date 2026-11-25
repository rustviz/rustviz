// `match r.as_str()` with r re-pointed in one arm: r's last use is the
// scrutinee, so its loan of s ends before any arm's body runs, on every
// path (#1578). The `_` arm used s while it was still drawn borrowed.

fn read_lender_in_other_arm() {
    let mut s = String::from("ab");
    let t = String::from("t");
    let mut r: &String = &mut s;
    match r.as_str() {
        "a" => {
            r = &t;
        }
        _ => {
            println!("{}", s);
        }
    }
}

fn move_lender_in_other_arm() {
    let s = String::from("ab");
    let t = String::from("t");
    let mut r = &s;
    match r.as_str() {
        "a" => {
            r = &t;
        }
        _ => {
            drop(s);
        }
    }
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn downgraded(o: Option<String>) {
    let mut s = String::from("ab");
    let mut r = downgrade(&mut s);
    match r.as_str() {
        "hi" | "ab" => {
            r = o.as_ref().unwrap();
        }
        _ => {
            s.push('x');
        }
    }
}

fn main() {}
