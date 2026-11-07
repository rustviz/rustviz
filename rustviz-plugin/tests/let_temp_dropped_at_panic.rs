// #553 with a panic (#410): a `let`-extended temporary (#400) still
// alive at a panic is dropped as the panic unwinds. Every path out of
// `g` panics or returns, so no drop dot of the temporary may say "at
// the end of the block".
fn g(x: u32) -> usize {
    let r = &String::from("b");
    if x == 0 {
        panic!("zero");
    } else {
        return r.len();
    }
}

fn main() {
    g(1);
}
