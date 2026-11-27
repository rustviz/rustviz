fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s2 = String::from("hi");
    let mut o = maybe(s2.clone());
    match o {
        Some(ref x6) if x6.len() > 1 => {
            if o.is_some() {
            } else if o.is_some() {
            } else {
                if x6.is_empty() {
                    return;
                }
                let m2 = o.as_mut().unwrap();
            }
        }
        Some(_) => {}
        None => {}
    }
}
