fn main() {
    let mut v = vec![String::from("a")];
    {
        let o = v.last_mut();
        if let Some(e) = o {
            e.push('x');
        }
    }
    println!("{}", v.len());
    for e in v.iter_mut() {
        e.push('z');
    }
}
