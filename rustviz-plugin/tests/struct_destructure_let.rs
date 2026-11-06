struct User { name: String, email: String, age: u32 }
struct Pair(String, String);

fn main() {
    let user = User { name: String::from("ann"), email: String::from("a@x"), age: 30 };
    let User { name, age, .. } = user;
    println!("{} {} {}", name, age, user.email);

    let pair = Pair(String::from("l"), String::from("r"));
    let Pair(left, right) = pair;
    println!("{} {}", left, right);
}
