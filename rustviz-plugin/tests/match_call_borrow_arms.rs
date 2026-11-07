fn as_mut_match() {
    let mut x = Some(String::from("a"));
    match x.as_mut() {
        Some(a) => a.push('!'),
        None => println!("no x"),
    }
}

fn get_mut_match() {
    let mut v = vec![String::from("b")];
    match v.get_mut(0) {
        Some(b) => b.push('!'),
        None => println!("no v"),
    }
}

fn iter_mut_match() {
    let mut w = vec![String::from("c")];
    match w.iter_mut().next() {
        Some(c) => c.push('!'),
        None => println!("no w"),
    }
}

fn as_ref_match() {
    let t = Some(String::from("d"));
    match t.as_ref() {
        Some(d) => println!("{d}"),
        None => println!("no t"),
    }
}

fn main() {
    as_mut_match();
    get_mut_match();
    iter_mut_match();
    as_ref_match();
}
