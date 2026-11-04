// A `&mut` passed where the parameter is a generic `T` is moved into the
// call: rustc reborrows a `&mut` argument only when the parameter's type
// is itself a reference, and `&mut String` isn't Copy (#200).

fn take<T>(_t: T) {}

fn push(x: &mut String) {
    x.push_str("!");
}

fn main() {
    let mut s = String::from("s");
    let y = &mut s;
    push(y);        // reborrow: y is still usable afterwards
    take(y);        // move: using y after this is E0382
    println!("{}", s);
}
