// #1512: a reference parameter pushed into a Vec that already borrows a local, in an arm.
fn run(r: &String, c: bool) {
    let s = String::from("hi");
    let mut vr = vec![&s];
    if c {
    } else {
        vr.push(r);
    }
}
fn as_str(r: &String, c: bool) {
    let s = String::from("hi");
    let mut vr = vec![s.as_str()];
    if c {
    } else {
        vr.push(r.as_str());
    }
}
fn main() { run(&String::new(), true); as_str(&String::new(), true); }
