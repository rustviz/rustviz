fn make() -> String {
    String::from("made")
}

fn mut_gen<T>(_t: &mut T) {}

fn run(s: String, mut o: Option<String>, flag: bool) -> Option<usize> {
    let s2 = String::from("rust");
    let mut s3 = String::from("ab");
    let mut tp = (format!("{}!", s2), 1);
    if let Some(x) = o {
        println!("{} {:?}", x, tp);
        o = None;
    } else {
        let c = move || s2.len();
    }
    let mut c2 = || s3.push('!');
    let mut c3 = || s3.push('!');
    mut_gen(&mut s3);
    o = Some(make());
    o = None;
    Some(0)
}
