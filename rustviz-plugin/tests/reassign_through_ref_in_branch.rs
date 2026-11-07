// #267: `*r = …` in an arm drops the value r's lender held before, on
// the lender's column (it used to get no branch at all).
fn main() {
    let mut t = String::new();
    let r = &mut t;
    let c = true;
    if c {
        *r = String::from("a");
    } else {
        *r = String::from("b");
    }
    println!("{}", t);
}
