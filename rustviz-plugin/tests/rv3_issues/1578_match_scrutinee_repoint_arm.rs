fn run() {
    let mut s = String::from("ab");
    let t = String::from("t");
    let mut r: &String = &mut s;
    match r.as_str() {
        "a" => {
            r = &t;
        }
        _ => {
            println!("{}", s);
        }
    }
}
fn main() {}
