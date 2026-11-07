// #506: reassigning a String says only that the new resource is acquired
// and the previous one dropped, not also a bare "acquires ownership".
fn main() {
    let mut s = String::from("a");
    s = String::from("b");
    println!("{}", s);
}
