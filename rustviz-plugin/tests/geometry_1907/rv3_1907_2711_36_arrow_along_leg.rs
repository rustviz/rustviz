fn take_gen<T>(_t: T) {}

fn run(mut o: Option<String>, mut v: Vec<String>, flag: bool) -> Option<usize> {
    let m2 = o.as_mut().unwrap();
    if let [x, rest @ ..] = v.as_slice() {
        if flag {
            take_gen(x);
            let m3 = m2;
        }
    }
    Some(0)
}
