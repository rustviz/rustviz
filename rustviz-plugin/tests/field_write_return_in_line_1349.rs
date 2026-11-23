// #1349: a value moved into a field through `&mut` on the row where the
// reference's borrow ends: "Move from a to r.name" points at r's dot and
// r's return to p leaves it on the other side. Drawn level, the two read
// as one arrow from a through r to p; the return takes another track.
struct P { name: String }

fn in_if_let() {
    let y = Some(String::from("bob"));
    let mut p = P { name: String::from("al") };
    let r = &mut p;
    if let Some(a) = y {
        r.name = a;
    }
    println!("{}", p.name);
}

fn in_match_arm() {
    let y = Some(String::from("bob"));
    let mut p = P { name: String::from("al") };
    let r = &mut p;
    match y {
        Some(a) => r.name = a,
        None => {}
    }
    println!("{}", p.name);
}

fn straight() {
    let mut p = P { name: String::from("al") };
    let r = &mut p;
    let a = String::from("bob");
    r.name = a;
    println!("{}", p.name);
}

fn main() {
    in_if_let();
    in_match_arm();
    straight();
}
