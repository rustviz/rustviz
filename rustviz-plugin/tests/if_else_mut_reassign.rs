// `let mut s` with branches that consume + reassign. Each arm
// moves `s` and immediately rebinds it from a fresh String, so at
// the merge `s` again owns a resource. The mutable binding means
// the column should render solid (single 5px line), not hollow
// (two parallel lines) — matching the regular column's
// determine_owner_line_styles logic for `(FullPrivilege, mut=true)`.

fn consume(_s: String) {}

fn main() {
    let mut s = String::from("orig");
    let cond = true;
    if cond {
        consume(s);
        s = String::from("new");
    } else {
        consume(s);
        s = String::from("alt");
    }
    println!("{}", s);
}
