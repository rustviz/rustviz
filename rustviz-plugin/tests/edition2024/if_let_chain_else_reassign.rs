// #281: when the else branch reassigns `x`, its old value was None or
// the condition has already moved the Some payload out. The scope-end
// drop still tests a flag for the payload the else branch stores.

fn main() {
    let mut x = Some(String::from("a"));
    if let Some(a) = x && a.len() > 5 {
        println!("{}", a);
    } else {
        x = Some(String::from("b"));
        println!("{:?}", x);
    }
}
