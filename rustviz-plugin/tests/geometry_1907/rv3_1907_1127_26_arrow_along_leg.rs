fn wrap_mut<'a>(s: &'a mut String) -> &'a mut String {
    s
}

fn main() {
    let mut s2 = String::from("abc");
    let o = Some(s2.to_uppercase());
    let m4 = wrap_mut(&mut s2);
    if let Some(x3) = &o && x3.len() > 1 {
        if m4.len() > 2 {
            m4.push('!');
        } else {
        }
    } else {
    }
}
