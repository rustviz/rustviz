fn sink(_b: bool, _u: ()) {}
fn mark() {}
pub fn f() {
    sink({ String::from("x").is_empty() }, mark());
}
