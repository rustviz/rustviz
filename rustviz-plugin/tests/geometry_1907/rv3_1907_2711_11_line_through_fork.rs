struct Pair {
    a: String,
    b: String,
}

impl Pair {
    fn get_a(&self) -> &String {
        &self.a
    }
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s = String::from("abc");
    match s.as_str() {
        "hi" | "ab" => {
        }
        _ => {
        }
    }
    let p = Pair { a: s, b: String::from("hi") };
    let r = p.get_a();
    let Some(x) = maybe(r.to_string()) else {
        return;
    };
    let mut x = x;
}
