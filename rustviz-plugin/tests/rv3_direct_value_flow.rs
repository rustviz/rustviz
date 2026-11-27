#![allow(dead_code)]

fn take<T>(_: T) {}
fn touch(text: &mut String) {
    text.push('!');
}
fn answer() -> usize {
    42
}

fn arguments(shared: &String, reuse: &mut String, moved: &mut String) {
    take(shared);
    take(shared);
    touch(reuse);
    reuse.push('?');
    take(moved);
}

fn forward() -> usize {
    answer()
}

fn build(owned: String) -> Result<String, ()> {
    Ok(owned)
}

fn propagate(pending: Result<String, ()>) -> Result<String, ()> {
    Ok(pending?)
}

fn stacked(left: String, right: String) -> Option<(String, usize, String)> {
    Some((left, answer(), right))
}

fn reborrow_result(reuse: &mut String) -> &mut String {
    reuse
}

#[allow(non_snake_case)]
fn Make() -> usize {
    answer()
}

fn receive_named_call() {
    let n = Make();
    std::hint::black_box(n);
}

fn receive_then_pass() {
    let held = answer(); take(held);
}
