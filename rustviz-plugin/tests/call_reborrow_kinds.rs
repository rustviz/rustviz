// What passing a reference to a function does to it depends on the
// parameter type, not just on the argument's type.

fn push(x: &mut String) {
    x.push_str("!");
}

fn first(x: &mut String) -> &mut String {
    x
}

fn show(x: &String) {}

fn take<T>(t: T) {}

fn main() {
    let mut s = String::from("s");
    let y = &mut s;
    push(y);                // implicit reborrow: y frozen until push returns
    let w = first(y);       // reborrow kept alive by w, past first's return
    w.push_str("?");
    take(y);                // generic T: y is moved, not reborrowed

    let t = String::from("t");
    let x = &t;
    show(x);                // &String is Copy: x is copied, not reborrowed
}
