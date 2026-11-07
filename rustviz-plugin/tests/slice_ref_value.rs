// A reference to a slice of Copy values or to a `str` points to a
// "value", not a "resource": nothing there needs dropping (#549).
// `&[String]` and `&String` keep "resource".
fn total(xs: &[i32]) -> i32 {
    let first = &xs[0];
    xs.len() as i32 + *first
}

fn shout(s: &str) -> usize {
    let t = &s[0..1];
    t.len() + s.len()
}

fn count(names: &[String]) -> usize {
    let first = &names[0];
    first.len() + names.len()
}

fn len_of(st: &String) -> usize {
    let t = &st[..];
    t.len() + st.len()
}

fn main() {
    let a = [1, 2, 3];
    let n = total(&a);
    let w = String::from("hi");
    let m = shout(&w);
    let v = vec![String::from("a")];
    let c = count(&v);
    let l = len_of(&w);
}
