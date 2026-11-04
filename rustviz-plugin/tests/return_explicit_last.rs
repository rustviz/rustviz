// #318: an explicit `return` as the last statement hands the value to
// the caller, like a tail expression.
struct User {
    name: String,
    age: u32,
}

fn make() -> String {
    let s = String::from("a");
    return s;
}

fn num() -> i32 {
    let n = 5;
    return n
}

fn build(name: String) -> User {
    let age = 3;
    return User { name, age };
}

fn main() {
    let r = make();
    let k = num();
    let u = build(String::from("b"));
    println!("{} {} {} {}", r, k, u.name, u.age);
}
