// #458: a write to a place that has no column of its own (a tuple
// field, a field reached through `v.last_mut().unwrap()`) is drawn on the
// column of the variable the place is reached from, and says the old
// value is dropped when its type needs drop. Timing confirmed with a
// Drop-printing program: `t.0 = x` drops the old `t.0` right there.
struct Q {
    name: String,
}

fn main() {
    let mut t = (String::from("a"), 1);
    t.0 = String::from("b");
    t.1 = 2;
    let n = 3;
    t.1 = n;
    let mut v = vec![Q { name: String::from("a") }];
    v.last_mut().unwrap().name = String::from("b");
    let s = String::from("s");
    let mut u = (String::from("u"), 0);
    u.0 = s;
    println!("{} {} {} {}", t.0, t.1, v[0].name, u.0);
}
