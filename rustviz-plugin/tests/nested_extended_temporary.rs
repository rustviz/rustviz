// #856: a temporary borrowed inside a `let`'s initializer, by a `&` in a
// struct literal's field, a tuple's element or a constructor's argument,
// lives until the end of the enclosing block (temporary lifetime
// extension), like #400's `let r = &String::from("a");`. It gets a
// `(temporary N)` column that the field borrows from and that is dropped
// at scope end, or where an early `return` leaves. `show(&..)`'s
// temporary is still dropped at the end of its statement.
struct W<'a> {
    r: &'a String,
}

fn show(s: &str) {
    println!("{}", s);
}

fn first_len(flag: bool) -> usize {
    let k = W { r: &String::from("k") };
    if flag {
        return k.r.len();
    }
    k.r.len()
}

fn main() {
    let w = W { r: &String::new() };
    let t = (&String::from("t"), 1);
    let s = Some(&String::from("s"));
    show(&String::from("gone"));
    println!("{} {} {:?} {}", w.r, t.0, s, first_len(true));
}
