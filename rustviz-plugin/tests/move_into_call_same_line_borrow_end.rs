// `s` is moved into `takes` on the line where r's borrow ends. The
// borrow's return arrow used to be the only arrow drawn on that line, so
// s's move vanished and s was shown dropping its String at the end.

fn takes(_s: String, _n: usize) {}

fn main() {
    let s = String::from("a");
    let t = String::from("b");
    let r = &t;
    takes(s, r.len());
}
