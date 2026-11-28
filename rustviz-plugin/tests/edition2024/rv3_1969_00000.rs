struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn make_pair() -> Pair {
    Pair { a: make(), b: make() }
}

fn take_gen<T>(_t: T) {}

fn main() {
    let mut s = String::from("ab");
    let mut s2 = String::from("ab");
    let p = make_pair();
    take_gen(&mut s);
    println!("{}", s2);
    let mut s3 = String::from("rust");
}
