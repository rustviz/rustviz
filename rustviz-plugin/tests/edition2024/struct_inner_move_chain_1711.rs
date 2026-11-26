struct Q { a: [Option<String>; 2] }
struct R { pair: [String; 2] }

fn in_chain(q: Q, c: bool) {
    if let [Some(s), _] = q.a && c {
        println!("{s}");
    }
}

fn take_both(r: R, c: bool) {
    if let [x, y] = r.pair && c {
        println!("{x} {y}");
    }
}
