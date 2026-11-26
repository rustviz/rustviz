// Known bug, from the closed 2.x fix PRs (cx/cx-text-b); kept as a RustViz 3 regression case.
// Issue(s): #1711 (see known_bugs/README.md for what main draws wrong).
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
