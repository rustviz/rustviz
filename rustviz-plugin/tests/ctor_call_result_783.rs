// #783: a call's result put into a constructor that is returned goes to
// the caller; a constructor is never the one that acts. `Some(make());`
// with nothing taking the value: the temporary takes what `make`
// returned, not the constructor.
fn check(x: i32) -> Result<i32, String> {
    if x < 0 {
        return Err(String::from("negative"));
    }
    Ok(1)
}

fn make() -> String {
    String::from("m")
}

fn wrap() -> Option<String> {
    Some(make())
}

fn boxed() -> Option<Box<i32>> {
    Some(Box::new(5))
}

fn temp() {
    Some(make());
}

fn main() {
    println!("{:?}", check(5));
    let _w = wrap();
    let _b = boxed();
    temp();
}
