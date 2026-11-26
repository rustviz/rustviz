struct Pair {
    word: String,
    count: i32,
}

fn partial_and_lent(mut pair: Pair) {
    let word = pair.word;
    let r = &mut pair.count;
    *r += 1;
    drop(word);
}

fn imaginary(mut pair: (String, String)) {
    let ((word, _) | (_, word)) = pair;
    println!("{word}");
    pair.1 = String::new();
}

fn shared(pair: Pair) {
    let r = &pair;
    println!("{}", r.count);
}

fn main() {}

fn deferred(a: String) {
    let value;
    if let f = a {
        value = f;
    } else {
        println!("dead");
    }
    println!("join");
}
