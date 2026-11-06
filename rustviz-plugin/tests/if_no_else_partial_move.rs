// #257: a struct field moved in an `if` without `else` is moved only
// when the condition holds.
struct Person {
    name: String,
    nick: String,
}

fn main() {
    let p = Person { name: String::from("Ann"), nick: String::from("A") };
    let c = true;
    if c {
        let n = p.name;
        println!("{}", n);
    }
    println!("{}", p.nick);
}
