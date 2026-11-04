// Deferred initialization in both branches of an if/else (#234). Each
// branch initializes s; after the if, s is definitely initialized and
// owns the String from whichever branch ran.

fn main() {
    let c = true;
    let s;
    if c {
        s = String::from("yes");
    } else {
        s = String::from("no");
    }
    println!("{}", s);
}
