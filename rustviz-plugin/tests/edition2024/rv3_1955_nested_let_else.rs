fn main() {
    let v = vec![String::new()];
    let Some(x) = v.first() else {
        let Some(x2) = v.first() else { return; };
        return;
    };
    println!("{x}");
}

fn slice(v: Vec<String>) -> Option<()> {
    let Some(first) = v.first() else {
        let [head, ..] = v.as_slice() else { return None; };
        return None;
    };
    println!("{first}");
    Some(())
}
