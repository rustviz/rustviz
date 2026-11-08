// An `Option<String>` moved in one branch and reassigned afterwards: the
// drop-flag message for the old value says what it holds (#679).

fn take(_o: Option<String>) {}

fn main() {
    let c = true;
    let mut o = Some(String::from("a"));
    if c {
        take(o);
    } else {
        println!("no");
    }
    o = Some(String::from("b"));
    println!("{:?}", o);
}
