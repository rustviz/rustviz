// #736: `take(*b)` runs in one branch only, so the String inside the
// Box may have been moved. Rust frees the Box's own heap allocation at
// scope end either way; the drop flag decides only the String.

fn take(s: String) -> bool { s.len() > 3 }

fn main() {
    let b = Box::new(String::from("a"));
    let c = true;
    if c {
        take(*b);
    } else {
        println!("no");
    }
}
