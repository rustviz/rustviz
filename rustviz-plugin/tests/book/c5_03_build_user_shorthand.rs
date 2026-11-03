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
