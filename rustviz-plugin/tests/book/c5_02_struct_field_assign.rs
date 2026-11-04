struct User {
    username: String,
    email: String,
}

fn main() {
    let mut user1 = User {
        username: String::from("someusername123"),
        email: String::from("someone@example.com"),
    };
    user1.email = String::from("anotheremail@example.com");
    println!("{} {}", user1.username, user1.email);
}
