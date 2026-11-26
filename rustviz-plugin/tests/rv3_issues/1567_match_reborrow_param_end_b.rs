fn run(m: &mut String, n: i32) {
    let r = &*m;
    match n {
        1 => {
            let k = r.len();
        }
        _ => {
            m.push('x');
        }
    }
}
