struct P { a: Option<String>, b: String }
fn see(_: &P) -> i32 { 1 }
fn two_phase(mut p: P) {
    let r = p.a.insert(
        String::from("x") + &see(&p).to_string(),
    );
    r.push('!');
}
