// Writing to a tuple element (#189): `t.0 = s` moves `s` into `t.0`
// and drops the String `t.0` held before; `t.1 = 5` overwrites an
// i32. Assigning a whole new tuple drops the old elements too.

fn main() {
    let s = String::from("x");
    let mut t = (String::new(), 1);
    t.0 = s;
    t.1 = 5;
    let w = String::from("w");
    t = (w, 2);
    println!("{:?}", t);
}
