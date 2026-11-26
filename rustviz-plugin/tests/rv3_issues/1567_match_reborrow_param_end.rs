fn run(p: &String, n: i32) {
    let r = &*p;
    match n {
        1 => {
            let k = r.len();
        }
        _ => {
            let k = p.len();
        }
    }
}
