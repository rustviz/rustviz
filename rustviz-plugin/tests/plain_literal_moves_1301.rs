// #1301: a struct with nothing to drop (`P { a: i32 }`, not Copy) put in a
// literal that nothing takes is moved into it all the same, and can't be
// used afterwards. The literal has nothing to drop either, so it gets no
// `(temporary)` column; main drew nothing at all on p. `let _ = p;` alone
// moves nothing (`_` doesn't bind), nor does a Copy value.
struct P { a: i32 }
struct W { p: P }
fn tuple() {
    let p = P { a: 1 };
    let _ = (p, 5);
}
fn array() {
    let q = P { a: 1 };
    let _ = [q];
}
fn strukt() {
    let u = P { a: 1 };
    let _ = W { p: u };
}
fn assign() {
    let v = P { a: 1 };
    _ = (v, 5);
}
fn stmt() {
    let w = P { a: 1 };
    (w, 5);
}
fn nested() {
    let x = P { a: 1 };
    let _ = ((x, 1), 2);
}
fn whole() {
    let y = P { a: 1 };
    let _ = y;
}
fn copy_tuple() {
    let t = (1, 2);
    let _ = (t, 5);
}
fn main() {
    tuple(); array(); strukt(); assign(); stmt(); nested(); whole(); copy_tuple();
}
