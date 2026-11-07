// #645: a `&mut` borrow made in a match arm that returns, and a lender
// declared in the returning branch itself.
fn arm(o: Option<u8>) -> usize {
    let mut t = String::from("t");
    match o {
        Some(_) => {
            let r = &mut t;
            r.push('x');
            return r.len();
        }
        None => println!("none"),
    }
    t.len()
}

fn local(c: bool) -> usize {
    let t = String::from("t");
    if c {
        let u = String::from("u");
        let r = &u;
        return r.len() + t.len();
    } else {
        println!("no");
    }
    t.len()
}

fn main() {
    println!("{} {}", arm(Some(1)), local(true));
}
