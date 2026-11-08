// `?` in a branch of an `if` with an `else` (#636): when `v` is empty,
// the `?` returns from `f`, dropping what `f` owns there: `u`, `t` and
// `v` (a Vec of Strings; `s` is a reference). Before, the branch's
// columns dropped the `?`'s events and no tooltip said so; without an
// `else` (`try_in_branch`) they were drawn.

fn f(v: Vec<String>, flag: bool) -> Option<usize> {
    let t = String::from("t");
    if flag {
        let u = String::from("u");
        let s = v.first()?;
        println!("{} {} {}", s, t, u);
    } else {
        println!("no {}", t);
    }
    Some(1)
}

fn main() {
    f(vec![String::from("a")], true);
}
