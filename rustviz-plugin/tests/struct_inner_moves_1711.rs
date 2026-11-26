struct P { a: [String; 2], q: Box<String>, z: String }

fn run(p: P) {
    let [x, _] = p.a;
    drop(*p.q);
    let n = 1;
}

fn in_arm(p: P, c: bool) {
    if c {
        let [y, _] = p.a;
        drop(*p.q);
        println!("{}", p.z);
    }
}

fn replace(mut p: P, next: P) {
    let [x, _] = p.a;
    drop(*p.q);
    p = next;
    drop(p);
}

fn early(p: P, opt: Option<String>) -> Option<String> {
    let [x, _] = p.a;
    drop(*p.q);
    let result = opt?;
    Some(result)
}
