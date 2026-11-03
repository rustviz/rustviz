// #174: `User { name, ..u }` returned from a fn: `name` and the
// non-overridden fields of `u` go to the caller; `u.name` (overridden)
// stays in `u` and is dropped at the end of `rename`.
struct User { name: String, email: String, active: bool }
fn rename(u: User, name: String) -> User {
    User { name, ..u }
}
fn main() {
    let u1 = User { name: String::from("a"), email: String::from("e"), active: true };
    let u2 = rename(u1, String::from("b"));
    println!("{} {}", u2.name, u2.email);
}
