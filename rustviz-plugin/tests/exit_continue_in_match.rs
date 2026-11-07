// The Book's guessing game: `Err(_) => continue` drops this
// iteration's `guess` (#410); `secret` was declared before the loop
// and isn't dropped there. The `break` in the `if` without `else`
// drops `guess` too (#429).

fn parse(s: &String) -> Result<u32, String> {
    if s.is_empty() {
        Err(String::from("empty"))
    } else {
        Ok(1)
    }
}

fn main() {
    let secret = String::from("42");
    loop {
        let guess = String::from("7");
        let n: u32 = match parse(&guess) {
            Ok(num) => num,
            Err(_) => continue,
        };
        if n > 0 {
            break;
        }
    }
    parse(&secret);
}
