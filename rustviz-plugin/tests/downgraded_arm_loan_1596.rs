fn downgrade(s: &mut String) -> &String {
    s
}

fn main() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = downgrade(&mut s2);
        let m2 = &mut s2;
    } else {
        m.push('a');
    }
}

fn assigned(mut text: String, c: bool) {
    let first = &mut text;
    if c {
        first.len();
        let result;
        result = downgrade(&mut text);
        result.len();
        text.clear();
    } else { first.clear(); }
}

fn id(s: &String) -> &String { s }

fn shares_one_loan(mut data: String, c: bool) {
    let origin = downgrade(&mut data);
    let copy = id(origin);
    if c {
        origin.len();
        let third = id(copy);
        third.len();
    }
    copy.len();
}
