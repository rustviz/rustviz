struct Shelf { items: Vec<String> }

fn run(v: Vec<String>, p: Shelf) {
    let empty = match &v[..] { [] => true, _ => false };
    match &p.items[..] {
        [] => {}
        [first, ..] => { println!("{first}"); }
    }
    match v[..] {
        [] => {}
        [ref first, ..] => { println!("{first}"); }
    }
}
