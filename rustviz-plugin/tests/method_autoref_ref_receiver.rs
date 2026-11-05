// A receiver that already is a reference isn't auto-referenced: for
// `r.len()` with `r: &String` and `w.push_str(..)` with `w: &mut String`,
// rustc derefs and reborrows (`&*r`, `&mut *w`) instead of borrowing a
// place. So no "(implicitly borrowed)" note; the `&mut` case keeps its
// reborrow note. `(*r).is_empty()` reborrows through a written `*`.
fn show(r: &String, w: &mut String) {
    let n = r.len();
    w.push_str("x");
    let k = (*r).is_empty();
    println!("{} {}", n, k);
}

fn main() {
    let a = String::from("a");
    let mut b = String::from("b");
    show(&a, &mut b);
}
