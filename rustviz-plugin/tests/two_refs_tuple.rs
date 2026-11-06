// Same as two_refs_same_line, with both borrows in one tuple (#233).

fn show(_r: &String) {}

fn main() {
    let x = String::from("x");
    let (a, b) = (&x, &x);
    show(a);
    show(b);
}
