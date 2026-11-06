// Tuples passed to and returned from functions (#189). `(s, 1)` as the
// tail moves `s` to the caller; `show((m, 2))` moves `m` into the
// call; `let u = t;` moves each element of `t` into `u`.

fn pair() -> (String, i32) {
    let s = String::from("a");
    (s, 1)
}

fn show(p: (String, i32)) {
    println!("{:?}", p);
}

fn main() {
    let t = pair();
    let u = t;
    let m = String::from("m");
    show((m, 2));
    println!("{:?}", u);
}
