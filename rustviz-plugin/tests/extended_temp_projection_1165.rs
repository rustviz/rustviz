struct User { name: String, age: u32 }
struct H<'a> { r: &'a String }
fn make_user() -> User { User { name: String::from("ann"), age: 3 } }
fn mk() -> Vec<String> { vec![String::from("a")] }
fn main() {
    let s = &String::from("hello")[1..];
    println!("{}", s);
    let name = &make_user().name;
    println!("{}", name);
    let age = &mut make_user().age;
    *age += 1;
    let e = &mk()[0];
    let h = H { r: &make_user().name };
    if *age > 0 {
        let inner = &String::from("in")[1..];
        println!("{}", inner);
    }
    println!("{} {} {}", age, e, h.r);
}
