// Five arms: the middle one sits on the column, two fan out to each
// side (#119).
fn consume(_s: String) {}
fn show(_s: &String) {}

fn main() {
    let s = String::from("hi");
    let n = 3;
    match n {
        0 => consume(s),
        1 => show(&s),
        2 => consume(s),
        3 => show(&s),
        _ => show(&s),
    }
}
