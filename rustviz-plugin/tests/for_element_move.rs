// #850: `for w in words` moves `words` into the loop's iterator; each
// iteration then moves one element (a String) out of that iterator into
// `w`. No arrow or dot says `w` takes the whole Vec.
fn consume(_s: String) {}

fn main() {
    let words = vec![String::from("hi"), String::from("ok")];
    for w in words {
        consume(w);
    }
}
