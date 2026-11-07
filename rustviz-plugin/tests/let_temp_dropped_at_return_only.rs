// #553: every path out of `f` is a `return`, so the `let`-extended
// temporary (#400) is only ever dropped at one: no drop dot of it may
// say "at the end of the block". (`let_temp_dropped_at_return` has a
// path that does reach the end of the block.)
fn f(x: u32) -> usize {
    let r = &String::from("a");
    if x == 0 {
        return 0;
    } else {
        return r.len() + 1;
    }
}

fn main() {
    f(1);
}
