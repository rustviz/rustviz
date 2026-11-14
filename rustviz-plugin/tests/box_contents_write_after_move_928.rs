// #928: `*b = …` after b's contents were moved out (`let w = *b;`).
// Moved on every path: the write drops nothing (the old String is gone).
// Moved in some branches only: the old String is dropped at the write
// only if the branch that ran didn't move it (a runtime drop flag). The
// Box keeps its heap allocation either way. Both writes said "its
// previous resource is dropped" (checked with a `Drop`-printing type).

fn main() {
    let mut e = Box::new(String::from("a"));
    let w = *e;
    *e = String::from("b");
    println!("{} {}", e, w);

    let mut f = Box::new(String::from("f"));
    let k = 3;
    if k == 1 {
        let w2 = *f;
        drop(w2);
    }
    *f = String::from("f2");
    println!("{}", f);
}
