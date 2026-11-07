fn as_mut_else() {
    let mut x = Some(String::from("a"));
    if let Some(a) = x.as_mut() {
        a.push('!');
    } else {
        println!("no x");
    }
}

fn get_mut_else() {
    let mut v = vec![String::from("b")];
    if let Some(b) = v.get_mut(0) {
        b.push('!');
    } else {
        println!("no v");
    }
}

fn iter_mut_else() {
    let mut w = vec![String::from("c")];
    if let Some(c) = w.iter_mut().next() {
        c.push('!');
    } else {
        println!("no w");
    }
}

fn as_ref_else() {
    let t = Some(String::from("d"));
    if let Some(d) = t.as_ref() {
        println!("{d}");
    } else {
        println!("no t");
    }
}

fn main() {
    as_mut_else();
    get_mut_else();
    iter_mut_else();
    as_ref_else();
}
