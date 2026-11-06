// A struct with a field moved on some paths only, where nothing needs drop
// (#484): the scope-end tooltip must not say anything is dropped.
struct It { a: std::slice::Iter<'static, i32>, b: std::slice::Iter<'static, i32> }
fn take(_i: std::slice::Iter<'static, i32>) {}
fn main() {
    let c = true;
    let it = [1, 2, 3].iter();
    let w = It { a: [1].iter(), b: [2].iter() };
    if c {
        take(it);
        take(w.a);
    }
}
