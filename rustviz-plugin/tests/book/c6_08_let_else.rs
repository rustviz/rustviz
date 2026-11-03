fn describe(s: Option<String>) -> String {
    let Some(x) = s else {
        return String::from("none");
    };
    x
}

fn main() {
    let d = describe(Some(String::from("a")));
    println!("{}", d);
}
