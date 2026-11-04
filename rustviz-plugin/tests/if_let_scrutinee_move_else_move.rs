// Same as `if_let_scrutinee_move`, plus a variable (`t`) that really is
// moved in one branch only (#450): `v` is moved before the conditional,
// on the parent column; only `t` is drawn as conditional.
fn head(items: Vec<String>) -> Option<String> {
    items.into_iter().next()
}

fn main() {
    let v = vec![String::from("a")];
    let t = String::from("t");
    if let Some(s) = head(v) {
        println!("{}", s);
    } else {
        drop(t);
    }
}
