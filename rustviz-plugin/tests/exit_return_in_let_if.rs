// A `return` inside the value of `let s = if … { … } else { … };`
// leaves before `s` gets a value, so only `t` is dropped there, not
// `s` (#410). `s` is bound once the `if` has finished; the bind of each
// branch's value to `s` is drawn in that branch, but it isn't on the
// path to the `return`.

fn f(c: bool, d: bool) -> usize {
    let t = String::from("t");
    let s = if c {
        if d {
            return 0;
        } else {
            println!("{}", t);
        }
        String::from("a")
    } else {
        String::from("b")
    };
    s.len() + t.len()
}

fn main() {
    println!("{}", f(true, false));
}
