// #674: `return if … { s } else { t };` returns what the branch that
// runs yields; the other variable is dropped when the function returns.
fn either(b: bool) -> String {
    let s = String::from("s");
    let t = String::from("t");
    return if b { s } else { t };
}

fn early(c: bool, b: bool) -> String {
    let p = String::from("p");
    let q = String::from("q");
    if c {
        println!("returning early");
        return if b { p } else { q };
    }
    String::from("end")
}

fn main() {
    let e = either(true);
    let f = early(true, false);
    println!("{} {}", e, f);
}
