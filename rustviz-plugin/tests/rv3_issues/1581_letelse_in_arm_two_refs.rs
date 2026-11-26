fn run(r: &String, m: &mut String, o2: Option<String>, k: u8) {
    match k {
        0 => {
            let Some(x2) = o2 else {
                let t = std::mem::replace(m, String::new());
                return;
            };
        }
        _ => {}
    }
}
