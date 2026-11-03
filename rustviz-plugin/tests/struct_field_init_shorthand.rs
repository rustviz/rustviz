// #174: field-init shorthand `email` moves the parameter `email` into
// the returned struct, so `email` drops nothing at the end of
// build_user; the caller's `u.email` owns (and drops) it.
struct User {
    active: bool,
    username: String,
    email: String,
}

fn build_user(email: String, username: String) -> User {
    User {
        active: true,
        username,
        email,
    }
}

fn main() {
    let u = build_user(String::from("a@b.c"), String::from("ab"));
    println!("{} {} {}", u.active, u.username, u.email);
}
