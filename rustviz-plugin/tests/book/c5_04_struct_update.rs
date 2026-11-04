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
