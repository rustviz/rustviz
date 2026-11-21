// #1185: a constructor wrapping a tuple, array or `vec![]` literal as the
// returned value. s goes to the caller inside the value, as for
// `Some(s)`; the constructor isn't an actor and receives nothing (#768).
fn wrap(s: String) -> Option<String> {
    Some(s)
}
fn wrap_tuple(s: String) -> Option<(String, i32)> {
    Some((s, 1))
}
fn wrap_vec(s: String) -> Option<Vec<String>> {
    Some(vec![s])
}
fn wrap_array(s: String, t: String) -> Option<[String; 2]> {
    return Some([s, t]);
}
fn ok_tuple(s: String) -> Result<(String, i32), ()> {
    Ok((s, 1))
}
fn main() {
    let _a = wrap(String::from("a"));
    let _b = wrap_tuple(String::from("b"));
    let _c = wrap_vec(String::from("c"));
    let _d = wrap_array(String::from("d"), String::from("e"));
    let _e = ok_tuple(String::from("f"));
}
