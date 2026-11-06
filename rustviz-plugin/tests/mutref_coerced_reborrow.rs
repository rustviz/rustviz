// Where the expected type is already a `&mut` (an annotated `let`, a
// struct field, an assignment), rustc coerces a `&mut` variable with an
// implicit reborrow (`&mut *a`) instead of moving it, so the variable is
// usable again once the new reference is done (#289).

struct Holder<'a> {
    r: &'a mut String,
}

fn main() {
    let mut s = String::from("hi");
    let r = &mut s;
    let r2: &mut String = r;   // reborrow, not a move
    r2.push('!');
    r.push('?');               // r is usable again

    let mut t = String::from("t");
    let a = &mut t;
    let h = Holder { r: a };   // reborrow into the field
    h.r.push('?');
    a.push('!');
    println!("{} {}", s, t);
}
