struct O { opt: Option<String> }
fn make_o() -> O { O { opt: None } }
fn temp() {
    let s = String::new();
    let Some(x) = make_o().opt else { return; };
}
