fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let mut o = maybe(String::from("hey"));
    if std::env::args_os().count() == 1 {
    } else {
        let m = o.as_mut().unwrap();
        if m.len() > 2 {
            let m2 = &mut *m; m2.push('!');
        } else {
        }
    }
    println!("{:?}", o);
    if let Some(x2) = o {
        o = Some(String::from("ab"));
        println!("{:?}", o);
    }
}
