fn keep<'a>(v: &mut Vec<&'a String>, s: &'a String) { v.push(s); }
fn f(n: usize) {
    let a = String::new(); let t = String::new();
    let mut refs = vec![&a];
    for i in 0..n {
        if i != 0 {
            println!("{t}");
        } else {
            keep(&mut refs, &t);
        }
    }
    println!("{}", refs.len());
}
