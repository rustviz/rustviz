// #603: `r.name = a` writes the field of the struct `r` points to; the
// else branch sees the old name or the new one.

struct P { name: String }

fn main() {
    let y = Some(String::from("bob"));
    let mut p = P { name: String::from("al") };
    let r = &mut p;
    if let Some(a) = y && { r.name = a; r.name.len() > 3 } {
        println!("long");
    } else {
        println!("short {}", r.name);
    }
}
