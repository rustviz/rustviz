// #1349's siblings in let-chains: a value written through `&mut` in a
// chain's block, on the row where the reference's borrow ends.
struct P { name: String }

fn deref_write() {
    let y = Some(String::from("bob"));
    let mut p = String::from("al");
    let r = &mut p;
    if let Some(a) = y && { *r = a; r.len() > 3 } {
        println!("long");
    } else {
        println!("short {}", r);
    }
}

fn while_chain() {
    let mut v = vec![String::from("bob")];
    let mut p = P { name: String::from("al") };
    let r = &mut p;
    while let Some(a) = v.pop() && { r.name = a; r.name.len() > 3 } {
        println!("long");
    }
    println!("{}", r.name);
}

fn main() {
    deref_write();
    while_chain();
}
