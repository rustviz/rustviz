fn make() -> String {
    String::from("made")
}

fn take(_s: String) {}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let mut s = String::from("abc");
    let mut s3 = make();
    let m = &mut s3;
    let s4 = std::mem::take(&mut s);
    let Some(x) = maybe(String::from("rust")) else {
        println!("{} {}", s4, s3);
        return;
    };
    match s4.as_str() {
        "hi" | "ab" => {
        }
        _ => {
            *m = format!("{}!", s);
        }
    }
}
