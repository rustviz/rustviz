// #174: `..user1` moves the fields not written out (`username`) and
// copies the Copy ones (`active`). `user1` is only partially moved:
// `user1.email` is still dropped at the end of main.
struct User {
    active: bool,
    username: String,
    email: String,
}

fn main() {
    let user1 = User {
        active: true,
        username: String::from("someusername123"),
        email: String::from("someone@example.com"),
    };
    let user2 = User {
        email: String::from("another@example.com"),
        ..user1
    };
    println!("{} {} {}", user2.active, user2.username, user1.email);
}
