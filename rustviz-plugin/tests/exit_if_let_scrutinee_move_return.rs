// An `if let` without `else` whose scrutinee moves a value and whose
// body returns (#429, #450): `v` is moved by `v.into_iter()` before
// the branch, on either path, so it owns nothing at its scope end;
// the `return` drops `t`, which goes on in the implicit `else`.

fn first(v: Vec<String>) -> usize {
    let t = String::from("t");
    if let Some(s) = v.into_iter().next() {
        return s.len();
    }
    t.len()
}

fn main() {
    println!("{}", first(vec![String::from("a")]));
}
