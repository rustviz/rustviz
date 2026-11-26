struct P { a: [String; 2], q: Box<String>, z: String }

fn run(p: P) {
    let [x, _] = p.a;
    drop(*p.q);
    let n = 1;
}
