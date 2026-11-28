fn take_gen<T>(_t: T) {}

fn id<T>(t: T) -> T {
    t
}

fn run(m: &mut String, v: Vec<String>, flag: bool) -> Option<usize> {
    let s = m.to_string();
    let s2 = s;
    let o2 = Some(id(s2));
    let m2 = &mut *m;
    if flag {
        match o2 {
            Some(x3) if x3.len() > 1 => {
            }
            Some(_) => {
                take_gen(&*m2);
            }
            None => {
            }
        }
        println!("{} {:?}", m, v);
    }
    Some(0)
}
