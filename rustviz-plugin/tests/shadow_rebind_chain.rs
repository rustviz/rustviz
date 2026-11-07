// Rebinding a name to change how it's used (#173): `let s = s;` moves
// the String into a new, immutable `s₂`, and `let s = &s;` makes `s₃`
// a reference borrowing from `s₂`.

fn main() {
    let mut s = String::from("a");
    s.push('b');
    let s = s;
    let s = &s;
    println!("{}", s);
}
