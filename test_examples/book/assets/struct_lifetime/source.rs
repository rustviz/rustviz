struct Excerpt<'a> {
    p: &'a str,
}

fn main() {
    let n = String::from("Ok. I'm fine.");
    let first = n.split('.').next().expect("Could not find a '.'");
    let i = Excerpt {
        p: first,
    };
}