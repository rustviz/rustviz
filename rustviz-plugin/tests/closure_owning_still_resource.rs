fn main() {
    let t = String::from("owned");
    let own = move || println!("{}", t);
    own();
    let u = String::from("dropped");
    let eat = || drop(u);
    eat();
}
