// #754: a reference parameter whose borrow flows into the returned
// reference through a reborrow (`&list[0]`, `for item in list`). The
// caller's `v` stays borrowed through `r` (rustc rejects `v.push(..)`
// before `r`'s last use), so list's borrow doesn't end inside `first`.
fn first(list: &[i32]) -> &i32 {
    let mut l = &list[0];
    for item in list {
        l = item;
    }
    l
}

fn main() {
    let v = vec![1, 2];
    let r = first(&v);
    println!("{r}");
}
